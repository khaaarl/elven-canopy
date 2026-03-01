#!/usr/bin/env python3
"""fix_tracker.py — Automated cleanup for docs/tracker.md.

Performs three categories of fixes:
  1. Alphabetizes summary lines within each status section (In Progress, Todo, Done).
  2. Alphabetizes detailed items within each topic group section.
  3. Strips Blocked-by/Blocks/Related references that point to done items from
     non-done items' Blocked-by and Blocks fields (done items can't block anything).
  4. Makes Blocks/Blocked-by symmetric: if A Blocks B, B gets Blocked-by A, and
     vice versa.
  5. Removes Blocks/Blocked-by/Related from done items (they're finished; the
     relationships are historical noise).

Run from repo root: python3 scripts/fix_tracker.py [--dry-run]
"""

import os
import re
import sys

TRACKER_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "tracker.md")

# ---------------------------------------------------------------------------
# Regex helpers
# ---------------------------------------------------------------------------

SUMMARY_LINE_RE = re.compile(r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)\s")
DETAIL_HEADING_RE = re.compile(r"^#### ([A-Za-z0-9][A-Za-z0-9-]*) —")
SECTION_HEADING_RE = re.compile(r"^### ")
H2_HEADING_RE = re.compile(r"^## ")
STATUS_LINE_RE = re.compile(r"^\*\*Status:\*\* (Done|Todo|In Progress)")

# Field lines like: **Blocks:** F-foo, F-bar
FIELD_RE = re.compile(r"^\*\*(Blocks|Blocked by|Related):\*\*\s*(.*)")


def parse_id_list(text):
    """Parse a comma-separated list of tracker IDs from a field value."""
    return [t.strip() for t in text.split(",") if t.strip()]


def format_id_list(ids):
    return ", ".join(sorted(set(ids)))


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------

class Item:
    def __init__(self, item_id, status):
        self.id = item_id
        self.status = status  # "Done", "Todo", "In Progress"
        self.blocks = []       # IDs this item blocks
        self.blocked_by = []   # IDs that block this item
        self.related = []      # Related IDs


# ---------------------------------------------------------------------------
# Parsing
# ---------------------------------------------------------------------------

def collect_items(lines):
    """Return dict of item_id -> Item, parsed from the detailed section."""
    items = {}
    current_id = None
    current_status = None

    for line in lines:
        m = DETAIL_HEADING_RE.match(line)
        if m:
            current_id = m.group(1)
            current_status = None
            items[current_id] = Item(current_id, "Unknown")
            continue

        if current_id is None:
            continue

        ms = STATUS_LINE_RE.match(line)
        if ms:
            items[current_id].status = ms.group(1)
            continue

        mf = FIELD_RE.match(line)
        if mf:
            field, value = mf.group(1), mf.group(2)
            ids = parse_id_list(value)
            if field == "Blocks":
                items[current_id].blocks = ids
            elif field == "Blocked by":
                items[current_id].blocked_by = ids
            elif field == "Related":
                items[current_id].related = ids

    return items


def compute_symmetric(items):
    """
    Given parsed items, compute the corrected Blocks/Blocked-by fields:
    - Remove references to done items from Blocks/Blocked-by (done items
      can't be blocking or blocked).
    - Make the graph symmetric: if A.blocks contains B, add A to B.blocked_by.
    - Remove Blocks/Blocked-by from done items entirely.
    Returns dict item_id -> (new_blocks, new_blocked_by, new_related).
    """
    done = {iid for iid, item in items.items() if item.status == "Done"}

    # Start from existing declarations, drop references to done items
    blocks = {}
    blocked_by = {}
    related = {}
    for iid, item in items.items():
        if item.status == "Done":
            blocks[iid] = []
            blocked_by[iid] = []
            related[iid] = list(item.related)
        else:
            blocks[iid] = [x for x in item.blocks if x not in done and x in items]
            blocked_by[iid] = [x for x in item.blocked_by if x not in done and x in items]
            related[iid] = list(item.related)  # keep related as-is for now

    # Symmetry pass: propagate blocks → blocked_by and blocked_by → blocks
    changed = True
    while changed:
        changed = False
        for iid in items:
            if items[iid].status == "Done":
                continue
            for target in list(blocks[iid]):
                if target in items and items[target].status != "Done":
                    if iid not in blocked_by[target]:
                        blocked_by[target].append(iid)
                        changed = True
            for source in list(blocked_by[iid]):
                if source in items and items[source].status != "Done":
                    if iid not in blocks[source]:
                        blocks[source].append(iid)
                        changed = True

    return blocks, blocked_by, related


# ---------------------------------------------------------------------------
# Rewriting
# ---------------------------------------------------------------------------

def rewrite_detail_fields(lines, blocks, blocked_by, related):
    """
    Rewrite Blocks/Blocked-by/Related field lines in the detailed section.
    For each item, replace existing field lines with correct values (sorted).
    If a field should now exist but didn't, insert it after the Status line.
    """
    out = []
    current_id = None
    # State machine: after we see a heading, we buffer the item's lines until
    # we hit the next heading or section break, then flush with corrections.
    item_buf = []  # lines belonging to current item

    def flush(item_id, buf):
        """Emit corrected lines for item_id."""
        if item_id is None:
            out.extend(buf)
            return

        new_blocks = sorted(set(blocks.get(item_id, [])))
        new_blocked_by = sorted(set(blocked_by.get(item_id, [])))
        new_related = related.get(item_id, [])

        # We'll rebuild the block: keep all non-field lines, then append fields
        # at the end (before the trailing blank line).
        non_field_lines = []
        has_status = False
        for l in buf:
            if FIELD_RE.match(l):
                continue  # drop old field lines
            non_field_lines.append(l)
            if STATUS_LINE_RE.match(l):
                has_status = True

        # Find position to insert fields: after the last non-blank non-field line
        # Remove trailing blank lines
        while non_field_lines and non_field_lines[-1].strip() == "":
            non_field_lines.pop()

        field_lines = []
        if new_blocked_by:
            field_lines.append(f"**Blocked by:** {format_id_list(new_blocked_by)}\n")
        if new_blocks:
            field_lines.append(f"**Blocks:** {format_id_list(new_blocks)}\n")
        if new_related:
            field_lines.append(f"**Related:** {format_id_list(new_related)}\n")

        out.extend(non_field_lines)
        if field_lines:
            out.append("\n")
            out.extend(field_lines)
        out.append("\n")

    i = 0
    in_detailed = False
    while i < len(lines):
        line = lines[i]

        if line.strip() == "## Detailed Items":
            in_detailed = True
            out.append(line)
            i += 1
            continue

        if not in_detailed:
            out.append(line)
            i += 1
            continue

        m = DETAIL_HEADING_RE.match(line)
        if m:
            # Flush previous item
            flush(current_id, item_buf)
            current_id = m.group(1)
            item_buf = [line]
            i += 1
            continue

        # Section-level heading (###, ##) ends current item
        if SECTION_HEADING_RE.match(line) or H2_HEADING_RE.match(line):
            flush(current_id, item_buf)
            current_id = None
            item_buf = []
            out.append(line)
            i += 1
            continue

        if current_id is not None:
            item_buf.append(line)
        else:
            out.append(line)
        i += 1

    flush(current_id, item_buf)
    return out


# ---------------------------------------------------------------------------
# Summary alphabetization
# ---------------------------------------------------------------------------

def sort_summary_section(lines):
    """Sort lines inside ``` blocks within summary sections."""
    out = []
    in_code = False
    code_buf = []

    for line in lines:
        if line.strip() == "```" and not in_code:
            in_code = True
            out.append(line)
            code_buf = []
            continue
        if line.strip() == "```" and in_code:
            in_code = False
            code_buf.sort(key=lambda l: re.match(r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)", l).group(1).lower()
                           if re.match(r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)", l) else l)
            out.extend(code_buf)
            out.append(line)
            continue
        if in_code:
            code_buf.append(line)
        else:
            out.append(line)
    return out


# ---------------------------------------------------------------------------
# Detail section alphabetization
# ---------------------------------------------------------------------------

def sort_detail_sections(lines):
    """
    Within each ### topic group in Detailed Items, sort #### items alphabetically
    by ID. Preserves inter-item blank lines.
    """
    # Identify the range of Detailed Items section
    detail_start = None
    for i, l in enumerate(lines):
        if l.strip() == "## Detailed Items":
            detail_start = i
            break
    if detail_start is None:
        return lines

    before = lines[:detail_start + 1]
    rest = lines[detail_start + 1:]

    # Split rest into topic groups (split on ### headings)
    groups = []  # each entry: (header_lines, item_chunks)
    current_header = []
    current_items = []  # list of (id, [lines])
    current_item_id = None
    current_item_lines = []

    def push_item():
        nonlocal current_item_id, current_item_lines
        if current_item_id is not None:
            current_items.append((current_item_id, current_item_lines))
        current_item_id = None
        current_item_lines = []

    def push_group():
        nonlocal current_header, current_items
        push_item()
        groups.append((current_header, current_items))
        current_header = []
        current_items = []

    for line in rest:
        if SECTION_HEADING_RE.match(line) or H2_HEADING_RE.match(line):
            push_group()
            current_header = [line]
            continue
        m = DETAIL_HEADING_RE.match(line)
        if m:
            push_item()
            current_item_id = m.group(1)
            current_item_lines = [line]
            continue
        if current_item_id is not None:
            current_item_lines.append(line)
        else:
            current_header.append(line)

    push_group()

    out = list(before)
    for header_lines, items in groups:
        out.extend(header_lines)
        items.sort(key=lambda x: x[0].lower())
        for _, item_lines in items:
            out.extend(item_lines)

    return out


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    dry_run = "--dry-run" in sys.argv

    with open(TRACKER_PATH, "r", encoding="utf-8") as f:
        original = f.read()

    lines = original.splitlines(keepends=True)

    # 1. Parse items
    items = collect_items(lines)

    # 2. Compute symmetric, cleaned Blocks/Blocked-by
    blocks, blocked_by, related = compute_symmetric(items)

    # 3. Rewrite field lines
    lines = rewrite_detail_fields(lines, blocks, blocked_by, related)

    # 4. Sort summary code blocks
    lines = sort_summary_section(lines)

    # 5. Sort detail items within each topic group
    lines = sort_detail_sections(lines)

    result = "".join(lines)

    if dry_run:
        if result == original:
            print("No changes needed.")
        else:
            import difflib
            diff = difflib.unified_diff(
                original.splitlines(keepends=True),
                result.splitlines(keepends=True),
                fromfile="tracker.md (before)",
                tofile="tracker.md (after)",
            )
            sys.stdout.writelines(diff)
        return

    if result == original:
        print("tracker.md is already clean — no changes made.")
    else:
        with open(TRACKER_PATH, "w", encoding="utf-8") as f:
            f.write(result)
        print("tracker.md updated.")


if __name__ == "__main__":
    main()
