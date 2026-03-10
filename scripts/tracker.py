#!/usr/bin/env python3
"""tracker.py — CLI tool for managing docs/tracker.md.

Provides query commands (list, show, search) for reading tracker state without
loading the full file into context, and mutation commands (change-state, add,
edit-title, edit-description, block, unblock, relate, unrelate, fix) for
targeted edits. Mutation commands auto-run ``fix`` at the end to normalize
ordering and relationship symmetry.

Subsumes the functionality of the former ``fix_tracker.py``.

Run from repo root: python3 scripts/tracker.py <command> [args]
"""

import argparse
import difflib
import os
import re
import sys

TRACKER_PATH = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), "..", "docs", "tracker.md"
)

# ---------------------------------------------------------------------------
# Regex helpers
# ---------------------------------------------------------------------------

SUMMARY_LINE_RE = re.compile(r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)\s")
DETAIL_HEADING_RE = re.compile(r"^#### ([A-Za-z0-9][A-Za-z0-9-]*) — (.*)")
SECTION_HEADING_RE = re.compile(r"^### ")
H2_HEADING_RE = re.compile(r"^## ")
STATUS_LINE_RE = re.compile(r"^\*\*Status:\*\* (Done|Todo|In Progress)")

# Field lines like: **Blocks:** F-foo, F-bar
FIELD_RE = re.compile(r"^\*\*(Blocks|Blocked by|Related):\*\*\s*(.*)")

# Info field lines (not relationship fields)
INFO_FIELD_RE = re.compile(
    r"^\*\*(New files|Modified files|Crate|Draft):\*\*"
)

# Summary section headings
SUMMARY_SECTION_RE = re.compile(r"^### (In Progress|Todo|Done)\s*$")

# Valid ID pattern
ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9-]*$")

# Column width for summary lines: "[marker] ID" is padded to this width.
# Must be >= len("[x] ") + max ID length.  Currently 26 to fit IDs up to 22
# chars (e.g. "F-large-nav-tolerance").
SUMMARY_PAD = 26

# Status mappings
STATUS_TO_MARKER = {"Todo": "[ ]", "In Progress": "[~]", "Done": "[x]"}
MARKER_TO_STATUS = {v: k for k, v in STATUS_TO_MARKER.items()}
CLI_TO_STATUS = {"todo": "Todo", "progress": "In Progress", "done": "Done"}


def parse_id_list(text):
    """Parse a comma-separated list of tracker IDs from a field value."""
    return [t.strip() for t in text.split(",") if t.strip()]


def format_id_list(ids):
    return ", ".join(sorted(set(ids)))


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------


class Item:
    def __init__(self, item_id, status="Unknown"):
        self.id = item_id
        self.status = status
        self.title = ""
        self.blocks = []
        self.blocked_by = []
        self.related = []
        self.phase = ""
        self.refs = ""
        self.group = ""  # ### heading this item lives under
        # Line ranges in the file (0-based, inclusive start, exclusive end)
        self.detail_start = -1  # line of #### heading
        self.detail_end = -1  # first line after this item
        self.summary_line = -1  # line number in summary section


# ---------------------------------------------------------------------------
# Parsing
# ---------------------------------------------------------------------------


def read_tracker():
    with open(TRACKER_PATH, "r", encoding="utf-8") as f:
        return f.read()


def collect_items(lines):
    """Return dict of item_id -> Item, parsed from both summary and detail."""
    items = {}
    current_id = None
    current_group = ""
    in_detailed = False
    in_summary = False

    for i, line in enumerate(lines):
        # Track whether we're in the summary or detailed section
        if line.strip() == "## Detailed Items":
            in_detailed = True
            in_summary = False
            continue
        if H2_HEADING_RE.match(line) and "Detailed Items" not in line:
            in_detailed = False

        # Summary section tracking
        if line.strip() in ("## Summary", ):
            in_summary = True
            continue
        if in_summary and H2_HEADING_RE.match(line):
            in_summary = False

        # Parse summary lines
        if in_summary:
            m = SUMMARY_LINE_RE.match(line)
            if m:
                sid = m.group(1)
                if sid not in items:
                    items[sid] = Item(sid)
                items[sid].summary_line = i

        if not in_detailed:
            continue

        # Track group headings
        if SECTION_HEADING_RE.match(line) and not DETAIL_HEADING_RE.match(line):
            # Close previous item
            if current_id and current_id in items:
                items[current_id].detail_end = i
            current_id = None
            current_group = re.sub(r"^#{1,3}\s*", "", line.strip())
            continue

        m = DETAIL_HEADING_RE.match(line)
        if m:
            # Close previous item
            if current_id and current_id in items:
                items[current_id].detail_end = i
            current_id = m.group(1)
            title = m.group(2).strip()
            if current_id not in items:
                items[current_id] = Item(current_id)
            items[current_id].title = title
            items[current_id].group = current_group
            items[current_id].detail_start = i
            continue

        if current_id is None or current_id not in items:
            continue

        ms = STATUS_LINE_RE.match(line)
        if ms:
            items[current_id].status = ms.group(1)
            # Parse phase and refs from the same line
            phase_m = re.search(r"\*\*Phase:\*\*\s*([^·]+)", line)
            if phase_m:
                items[current_id].phase = phase_m.group(1).strip()
            refs_m = re.search(r"\*\*Refs:\*\*\s*(.+)$", line)
            if refs_m:
                items[current_id].refs = refs_m.group(1).strip()
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

    # Close last item
    if current_id and current_id in items:
        items[current_id].detail_end = len(lines)

    return items


def get_groups(lines):
    """Return ordered list of ### group names in the Detailed Items section."""
    groups = []
    in_detailed = False
    for line in lines:
        if line.strip() == "## Detailed Items":
            in_detailed = True
            continue
        if in_detailed and H2_HEADING_RE.match(line):
            break
        if in_detailed and SECTION_HEADING_RE.match(line):
            name = re.sub(r"^#{1,3}\s*", "", line.strip())
            groups.append(name)
    return groups


# ---------------------------------------------------------------------------
# Fix functions (migrated from fix_tracker.py)
# ---------------------------------------------------------------------------


def compute_symmetric(items):
    """Compute corrected Blocks/Blocked-by/Related fields with symmetry."""
    done = {iid for iid, item in items.items() if item.status == "Done"}

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
            blocked_by[iid] = [
                x for x in item.blocked_by if x not in done and x in items
            ]
            related[iid] = list(item.related)

    # Blocks/Blocked-by symmetry
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

    # Related symmetry: if A lists B as related, B should list A
    for iid in items:
        for target in list(related.get(iid, [])):
            if target in items and iid not in related.get(target, []):
                related.setdefault(target, []).append(iid)

    return blocks, blocked_by, related


def rewrite_detail_fields(lines, blocks, blocked_by, related):
    """Rewrite Blocks/Blocked-by/Related field lines in the detailed section."""
    out = []
    current_id = None
    item_buf = []

    def flush(item_id, buf):
        if item_id is None:
            out.extend(buf)
            return

        new_blocks = sorted(set(blocks.get(item_id, [])))
        new_blocked_by = sorted(set(blocked_by.get(item_id, [])))
        new_related = related.get(item_id, [])

        non_field_lines = []
        for l in buf:
            if FIELD_RE.match(l):
                continue
            non_field_lines.append(l)

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
            flush(current_id, item_buf)
            current_id = m.group(1)
            item_buf = [line]
            i += 1
            continue

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


def normalize_summary_lines(lines):
    """Reformat summary lines inside ``` blocks to consistent column alignment."""
    out = []
    in_code = False

    for line in lines:
        if line.strip() == "```" and not in_code:
            in_code = True
            out.append(line)
            continue
        if line.strip() == "```" and in_code:
            in_code = False
            out.append(line)
            continue
        if in_code:
            m = SUMMARY_LINE_RE.match(line)
            if m:
                item_id = m.group(1)
                # Extract marker and title from the raw line
                marker = line[:3]  # [x], [ ], [~]
                title = line[m.end():].strip()
                id_part = f"{marker} {item_id}"
                padded = id_part.ljust(SUMMARY_PAD)
                out.append(f"{padded} {title}\n")
            else:
                out.append(line)
        else:
            out.append(line)
    return out


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
            code_buf.sort(
                key=lambda l: re.match(
                    r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)", l
                ).group(1).lower()
                if re.match(r"^\[[ x~]\] ([A-Za-z0-9][A-Za-z0-9-]*)", l)
                else l
            )
            out.extend(code_buf)
            out.append(line)
            continue
        if in_code:
            code_buf.append(line)
        else:
            out.append(line)
    return out


def sort_detail_sections(lines):
    """Within each ### topic group, sort #### items alphabetically by ID."""
    detail_start = None
    for i, l in enumerate(lines):
        if l.strip() == "## Detailed Items":
            detail_start = i
            break
    if detail_start is None:
        return lines

    before = lines[: detail_start + 1]
    rest = lines[detail_start + 1 :]

    groups = []
    current_header = []
    current_items = []
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


def run_fix(lines):
    """Apply all fix passes to lines. Returns new list of lines."""
    items = collect_items(lines)
    blocks, blocked_by, related = compute_symmetric(items)
    lines = rewrite_detail_fields(lines, blocks, blocked_by, related)
    lines = normalize_summary_lines(lines)
    lines = sort_summary_section(lines)
    lines = sort_detail_sections(lines)
    return lines


# ---------------------------------------------------------------------------
# Query commands
# ---------------------------------------------------------------------------


def cmd_list(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    status_filter = args.status
    if status_filter == "all":
        wanted = {"Todo", "In Progress", "Done"}
    elif status_filter == "done":
        wanted = {"Done"}
    elif status_filter == "progress":
        wanted = {"In Progress"}
    elif status_filter == "todo":
        wanted = {"Todo"}
    else:
        # Default: progress + todo
        wanted = {"In Progress", "Todo"}

    # Group by status for display
    by_status = {"In Progress": [], "Todo": [], "Done": []}
    for iid, item in sorted(items.items(), key=lambda x: x[0].lower()):
        if item.status in wanted:
            by_status.setdefault(item.status, []).append(item)

    for status in ["In Progress", "Todo", "Done"]:
        group = by_status.get(status, [])
        if not group:
            continue
        print(f"### {status}")
        for item in group:
            marker = STATUS_TO_MARKER.get(item.status, "[ ]")
            id_part = f"{marker} {item.id}"
            padded = id_part.ljust(SUMMARY_PAD)
            print(f"{padded} {item.title}")
        print()


def cmd_show(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    for item_id in args.ids:
        if item_id not in items:
            print(f"Error: unknown item '{item_id}'", file=sys.stderr)
            sys.exit(1)
        item = items[item_id]
        if item.detail_start < 0:
            print(f"Error: no detail entry for '{item_id}'", file=sys.stderr)
            sys.exit(1)
        # Print the detail block
        for line in lines[item.detail_start : item.detail_end]:
            print(line, end="")
        print()


def cmd_search(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    flags = re.IGNORECASE if args.case_insensitive else 0
    # Normalize BRE-style escaped pipes to ERE-style alternation.
    raw = args.pattern.replace(r"\|", "|")
    try:
        pattern = re.compile(raw, flags)
    except re.error as e:
        print(f"Error: invalid regex: {e}", file=sys.stderr)
        sys.exit(1)

    matches = []
    for iid, item in sorted(items.items(), key=lambda x: x[0].lower()):
        # Search in summary line
        marker = STATUS_TO_MARKER.get(item.status, "[ ]")
        summary_text = f"{marker} {item.id} {item.title}"
        if pattern.search(summary_text):
            matches.append(item)
            continue
        # Search in detail text
        if item.detail_start >= 0:
            detail_text = "".join(lines[item.detail_start : item.detail_end])
            if pattern.search(detail_text):
                matches.append(item)

    if not matches:
        print("No matches found.")
        return

    for item in matches:
        marker = STATUS_TO_MARKER.get(item.status, "[ ]")
        id_part = f"{marker} {item.id}"
        padded = id_part.ljust(SUMMARY_PAD)
        print(f"{padded} {item.title}")


# ---------------------------------------------------------------------------
# Mutation helpers
# ---------------------------------------------------------------------------


def write_tracker(original, lines, dry_run):
    """Write lines back to tracker, showing diff in dry-run mode."""
    result = "".join(lines)
    if dry_run:
        if result == original:
            print("No changes needed.")
        else:
            diff = difflib.unified_diff(
                original.splitlines(keepends=True),
                result.splitlines(keepends=True),
                fromfile="tracker.md (before)",
                tofile="tracker.md (after)",
            )
            sys.stdout.writelines(diff)
        return
    if result == original:
        print("No changes needed.")
    else:
        with open(TRACKER_PATH, "w", encoding="utf-8") as f:
            f.write(result)
        print("tracker.md updated.")


def find_summary_sections(lines):
    """Return dict mapping status name to (start, end) line indices of the
    code block contents (inside the ``` markers) for each summary section."""
    sections = {}
    current_section = None
    in_code = False
    code_start = None

    for i, line in enumerate(lines):
        m = SUMMARY_SECTION_RE.match(line.strip())
        if m:
            current_section = m.group(1)
            continue
        if current_section and line.strip() == "```":
            if not in_code:
                in_code = True
                code_start = i + 1
            else:
                sections[current_section] = (code_start, i)
                in_code = False
                current_section = None

    return sections


def make_summary_line(item_id, title, status):
    """Create a formatted summary line."""
    marker = STATUS_TO_MARKER[status]
    id_part = f"{marker} {item_id}"
    padded = id_part.ljust(SUMMARY_PAD)
    return f"{padded} {title}\n"


def set_relationship_field(lines, item, field_name, new_ids):
    """Set a relationship field (Blocks/Blocked by/Related) on an item's
    detail entry. Adds, updates, or removes the field line as needed.
    Returns the modified lines list."""
    if item.detail_start < 0:
        return lines

    # Find the field line within the item's detail range
    field_line_idx = None
    for i in range(item.detail_start, item.detail_end):
        m = FIELD_RE.match(lines[i])
        if m and m.group(1) == field_name:
            field_line_idx = i
            break

    if new_ids:
        new_line = f"**{field_name}:** {format_id_list(new_ids)}\n"
        if field_line_idx is not None:
            lines[field_line_idx] = new_line
        else:
            # Insert before the trailing blank line(s) at the end of the item
            insert_at = item.detail_end
            while insert_at > item.detail_start and lines[insert_at - 1].strip() == "":
                insert_at -= 1
            lines.insert(insert_at, new_line)
    else:
        if field_line_idx is not None:
            lines.pop(field_line_idx)

    return lines


# ---------------------------------------------------------------------------
# Mutation commands
# ---------------------------------------------------------------------------


def cmd_change_state(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    item_id = args.id
    new_status = CLI_TO_STATUS.get(args.state)
    if new_status is None:
        print(f"Error: invalid state '{args.state}'. Use: todo, progress, done",
              file=sys.stderr)
        sys.exit(1)

    if item_id not in items:
        print(f"Error: unknown item '{item_id}'", file=sys.stderr)
        sys.exit(1)

    item = items[item_id]
    if item.detail_start < 0:
        print(f"Error: no detail entry for '{item_id}'", file=sys.stderr)
        sys.exit(1)
    if item.status == new_status:
        print(f"No change needed — {item_id} is already {new_status}.")
        return

    old_status = item.status

    # 1. Update detail status line
    for i in range(item.detail_start, item.detail_end):
        ms = STATUS_LINE_RE.match(lines[i])
        if ms:
            lines[i] = lines[i].replace(
                f"**Status:** {old_status}", f"**Status:** {new_status}"
            )
            break

    # 2. Update summary: remove from old section, add to new section
    sections = find_summary_sections(lines)

    # Find and remove the old summary line
    old_section = sections.get(old_status)
    summary_text = None
    if old_section:
        for i in range(old_section[0], old_section[1]):
            m = SUMMARY_LINE_RE.match(lines[i])
            if m and m.group(1) == item_id:
                # Extract the title from this line
                rest = lines[i][m.end():].strip()
                summary_text = rest
                lines.pop(i)
                break

    if summary_text is None:
        summary_text = item.title

    # Recalculate sections after the pop
    sections = find_summary_sections(lines)

    # Insert into the new section in alphabetical order
    new_section = sections.get(new_status)
    new_line = make_summary_line(item_id, summary_text, new_status)

    if new_section is None:
        print(
            f"Error: no '{new_status}' summary section found in tracker.",
            file=sys.stderr,
        )
        sys.exit(1)

    insert_idx = new_section[0]
    for i in range(new_section[0], new_section[1]):
        m = SUMMARY_LINE_RE.match(lines[i])
        if m and m.group(1).lower() > item_id.lower():
            insert_idx = i
            break
        else:
            insert_idx = i + 1
    # Handle empty section
    if new_section[0] == new_section[1]:
        insert_idx = new_section[0]
    lines.insert(insert_idx, new_line)

    # Run fix
    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_add(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)
    groups = get_groups(lines)

    item_id = args.id
    title = args.title
    group_name = args.group
    status = CLI_TO_STATUS.get(args.status, "Todo")
    phase = args.phase
    refs = args.refs

    # Validate ID
    if not ID_RE.match(item_id):
        print(
            f"Error: invalid ID '{item_id}'. Must match [A-Za-z0-9][A-Za-z0-9-]*, "
            f"max 20 chars.",
            file=sys.stderr,
        )
        sys.exit(1)
    if len(item_id) > 20:
        print(f"Error: ID '{item_id}' exceeds 20 characters.", file=sys.stderr)
        sys.exit(1)
    if item_id in items:
        print(f"Error: ID '{item_id}' already exists.", file=sys.stderr)
        sys.exit(1)

    # Validate group (case-insensitive match)
    matched_group = None
    for g in groups:
        if g.lower() == group_name.lower():
            matched_group = g
            break
    if matched_group is None:
        print(
            f"Error: unknown group '{group_name}'. Valid groups:",
            file=sys.stderr,
        )
        for g in groups:
            print(f"  - {g}", file=sys.stderr)
        sys.exit(1)

    # 1. Add summary line to the appropriate section
    sections = find_summary_sections(lines)
    target_section = sections.get(status)
    if target_section:
        new_summary = make_summary_line(item_id, title, status)
        insert_idx = target_section[0]
        for i in range(target_section[0], target_section[1]):
            m = SUMMARY_LINE_RE.match(lines[i])
            if m and m.group(1).lower() > item_id.lower():
                insert_idx = i
                break
            else:
                insert_idx = i + 1
        if target_section[0] == target_section[1]:
            insert_idx = target_section[0]
        lines.insert(insert_idx, new_summary)

    # 2. Add detail entry in the correct group
    # Find the group heading, then find the right alphabetical position
    group_heading_idx = None
    for i, line in enumerate(lines):
        if line.strip() == f"### {matched_group}":
            # Verify we're in the Detailed Items section
            in_detailed = False
            for j in range(i - 1, -1, -1):
                if lines[j].strip() == "## Detailed Items":
                    in_detailed = True
                    break
                if H2_HEADING_RE.match(lines[j]) and "Detailed Items" not in lines[j]:
                    break
            if in_detailed:
                group_heading_idx = i
                break

    if group_heading_idx is None:
        print(
            f"Error: could not locate group '{matched_group}' in Detailed Items.",
            file=sys.stderr,
        )
        sys.exit(1)

    # Build the detail entry
    status_line = f"**Status:** {status}"
    if phase:
        status_line += f" · **Phase:** {phase}"
    if refs:
        status_line += f" · **Refs:** {refs}"

    detail_lines = [
        f"#### {item_id} — {title}\n",
        f"{status_line}\n",
        "\n",
    ]

    # Find insertion point: scan #### headings after group_heading_idx until
    # we hit one with ID > ours, or the next ### / ## heading
    insert_at = None
    for i in range(group_heading_idx + 1, len(lines)):
        if SECTION_HEADING_RE.match(lines[i]) or H2_HEADING_RE.match(lines[i]):
            if not DETAIL_HEADING_RE.match(lines[i]):
                insert_at = i
                break
        m = DETAIL_HEADING_RE.match(lines[i])
        if m and m.group(1).lower() > item_id.lower():
            insert_at = i
            break

    if insert_at is None:
        insert_at = len(lines)

    for j, dl in enumerate(detail_lines):
        lines.insert(insert_at + j, dl)

    # Run fix
    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_edit_title(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    item_id = args.id
    new_title = args.title

    if item_id not in items:
        print(f"Error: unknown item '{item_id}'", file=sys.stderr)
        sys.exit(1)

    item = items[item_id]

    # 1. Update detail heading
    if item.detail_start >= 0:
        lines[item.detail_start] = f"#### {item_id} — {new_title}\n"

    # 2. Update summary line
    if item.summary_line >= 0:
        lines[item.summary_line] = make_summary_line(
            item_id, new_title, item.status
        )

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_edit_description(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    item_id = args.id
    if item_id not in items:
        print(f"Error: unknown item '{item_id}'", file=sys.stderr)
        sys.exit(1)

    item = items[item_id]
    if item.detail_start < 0:
        print(f"Error: no detail entry for '{item_id}'", file=sys.stderr)
        sys.exit(1)

    # Identify the prose block within the detail entry.
    # Skip: heading (line 0), status/meta line, standalone Draft line, leading blanks.
    # From the end, skip: trailing blanks, relationship fields, info fields.
    detail = lines[item.detail_start : item.detail_end]

    # Find prose start: right after the status/meta line and any info fields
    # (Draft, New files, etc.) that appear before prose. Include the blank
    # separator so we control whitespace on replacement.
    prose_start = 1  # skip heading
    while prose_start < len(detail):
        line = detail[prose_start]
        if STATUS_LINE_RE.match(line):
            prose_start += 1
            break
        prose_start += 1
    # Skip standalone info fields (Draft, New files, etc.) after status line
    while prose_start < len(detail) and INFO_FIELD_RE.match(detail[prose_start]):
        prose_start += 1

    # Find prose end: scan backward from end, skip blanks, fields, info fields
    prose_end = len(detail)
    while prose_end > prose_start:
        line = detail[prose_end - 1]
        if line.strip() == "":
            prose_end -= 1
            continue
        if FIELD_RE.match(line):
            prose_end -= 1
            continue
        if INFO_FIELD_RE.match(line):
            prose_end -= 1
            continue
        break

    # Get new text from file
    with open(args.file, "r", encoding="utf-8") as f:
        new_text = f.read()

    # Build replacement: blank separator line, then prose lines
    new_text = new_text.rstrip("\n")
    new_prose_lines = ["\n"]
    for l in new_text.split("\n"):
        new_prose_lines.append(l + "\n")

    # Replace prose block
    abs_start = item.detail_start + prose_start
    abs_end = item.detail_start + prose_end
    lines[abs_start:abs_end] = new_prose_lines

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_block(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    item_id = args.id
    blocker_id = args.by

    for check_id in (item_id, blocker_id):
        if check_id not in items:
            print(f"Error: unknown item '{check_id}'", file=sys.stderr)
            sys.exit(1)

    item = items[item_id]
    if blocker_id in item.blocked_by:
        print(f"No change needed — {item_id} is already blocked by {blocker_id}.")
        return

    # Add to blocked_by (fix will handle symmetry)
    new_blocked_by = item.blocked_by + [blocker_id]
    lines = set_relationship_field(lines, item, "Blocked by", new_blocked_by)

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_unblock(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    item_id = args.id
    blocker_id = args.by

    if item_id not in items:
        print(f"Error: unknown item '{item_id}'", file=sys.stderr)
        sys.exit(1)

    item = items[item_id]
    if blocker_id not in item.blocked_by:
        print(f"No change needed — {item_id} is not blocked by {blocker_id}.")
        return

    new_blocked_by = [x for x in item.blocked_by if x != blocker_id]
    lines = set_relationship_field(lines, item, "Blocked by", new_blocked_by)

    # Also remove from blocker's Blocks field
    if blocker_id in items:
        # Re-parse since lines changed
        items2 = collect_items(lines)
        if blocker_id in items2:
            blocker = items2[blocker_id]
            new_blocks = [x for x in blocker.blocks if x != item_id]
            lines = set_relationship_field(lines, blocker, "Blocks", new_blocks)

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_relate(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    id_a, id_b = args.id1, args.id2
    for check_id in (id_a, id_b):
        if check_id not in items:
            print(f"Error: unknown item '{check_id}'", file=sys.stderr)
            sys.exit(1)

    item_a = items[id_a]
    item_b = items[id_b]

    if id_b in item_a.related and id_a in item_b.related:
        print(f"No change needed — {id_a} and {id_b} are already related.")
        return

    # Add bidirectional relationship
    if id_b not in item_a.related:
        new_related_a = item_a.related + [id_b]
        lines = set_relationship_field(lines, item_a, "Related", new_related_a)

    # Re-parse for updated line numbers
    items2 = collect_items(lines)
    if id_b in items2:
        item_b2 = items2[id_b]
        if id_a not in item_b2.related:
            new_related_b = item_b2.related + [id_a]
            lines = set_relationship_field(lines, item_b2, "Related", new_related_b)

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_unrelate(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    items = collect_items(lines)

    id_a, id_b = args.id1, args.id2
    for check_id in (id_a, id_b):
        if check_id not in items:
            print(f"Error: unknown item '{check_id}'", file=sys.stderr)
            sys.exit(1)

    item_a = items[id_a]
    item_b = items[id_b]

    if id_b not in item_a.related and id_a not in item_b.related:
        print(f"No change needed — {id_a} and {id_b} are not related.")
        return

    if id_b in item_a.related:
        new_related_a = [x for x in item_a.related if x != id_b]
        lines = set_relationship_field(lines, item_a, "Related", new_related_a)

    items2 = collect_items(lines)
    if id_b in items2:
        item_b2 = items2[id_b]
        if id_a in item_b2.related:
            new_related_b = [x for x in item_b2.related if x != id_a]
            lines = set_relationship_field(lines, item_b2, "Related", new_related_b)

    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


def cmd_fix(args):
    original = read_tracker()
    lines = original.splitlines(keepends=True)
    lines = run_fix(lines)
    write_tracker(original, lines, args.dry_run)


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def build_parser():
    parser = argparse.ArgumentParser(
        prog="tracker.py",
        description="CLI tool for managing docs/tracker.md",
    )
    sub = parser.add_subparsers(dest="command")

    # list
    p_list = sub.add_parser("list", help="List summary lines by status")
    p_list.add_argument(
        "--status",
        choices=["todo", "progress", "done", "all"],
        default=None,
        help="Filter by status (default: progress + todo)",
    )

    # show
    p_show = sub.add_parser("show", help="Print full detailed entries")
    p_show.add_argument("ids", nargs="+", metavar="ID", help="Item ID(s)")

    # search
    p_search = sub.add_parser("search", help="Regex search across items")
    p_search.add_argument("pattern", help="Regex pattern")
    p_search.add_argument(
        "-i", dest="case_insensitive", action="store_true",
        help="Case-insensitive search",
    )

    # change-state
    p_cs = sub.add_parser("change-state", help="Move item to todo/progress/done")
    p_cs.add_argument("id", metavar="ID", help="Item ID")
    p_cs.add_argument(
        "state", choices=["todo", "progress", "done"], help="New state"
    )
    p_cs.add_argument("--dry-run", action="store_true")

    # add
    p_add = sub.add_parser("add", help="Add a new item")
    p_add.add_argument("id", metavar="ID", help="Item ID (e.g. F-my-feature)")
    p_add.add_argument("title", help="Short title")
    p_add.add_argument("--group", required=True, help="Topic group name")
    p_add.add_argument("--phase", default="", help="Phase number")
    p_add.add_argument("--refs", default="", help="Design doc refs (e.g. §11)")
    p_add.add_argument(
        "--status",
        choices=["todo", "progress", "done"],
        default="todo",
        help="Initial status (default: todo)",
    )
    p_add.add_argument("--dry-run", action="store_true")

    # edit-title
    p_et = sub.add_parser("edit-title", help="Change title")
    p_et.add_argument("id", metavar="ID", help="Item ID")
    p_et.add_argument("title", help="New title")
    p_et.add_argument("--dry-run", action="store_true")

    # edit-description
    p_ed = sub.add_parser("edit-description", help="Set/replace prose description")
    p_ed.add_argument("id", metavar="ID", help="Item ID")
    p_ed.add_argument("file", metavar="FILE", help="File to read description from")
    p_ed.add_argument("--dry-run", action="store_true")

    # block
    p_block = sub.add_parser("block", help="Add blocked-by relationship")
    p_block.add_argument("id", metavar="ID", help="Item that is blocked")
    p_block.add_argument("--by", required=True, help="Item that blocks it")
    p_block.add_argument("--dry-run", action="store_true")

    # unblock
    p_unblock = sub.add_parser("unblock", help="Remove blocked-by relationship")
    p_unblock.add_argument("id", metavar="ID", help="Item that was blocked")
    p_unblock.add_argument("--by", required=True, help="Item to unblock from")
    p_unblock.add_argument("--dry-run", action="store_true")

    # relate
    p_relate = sub.add_parser("relate", help="Add bidirectional related relationship")
    p_relate.add_argument("id1", metavar="ID1")
    p_relate.add_argument("id2", metavar="ID2")
    p_relate.add_argument("--dry-run", action="store_true")

    # unrelate
    p_unrelate = sub.add_parser(
        "unrelate", help="Remove bidirectional related relationship"
    )
    p_unrelate.add_argument("id1", metavar="ID1")
    p_unrelate.add_argument("id2", metavar="ID2")
    p_unrelate.add_argument("--dry-run", action="store_true")

    # fix
    p_fix = sub.add_parser("fix", help="Cleanup: sort, symmetrize, prune")
    p_fix.add_argument("--dry-run", action="store_true")

    return parser


def main():
    parser = build_parser()
    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(1)

    commands = {
        "list": cmd_list,
        "show": cmd_show,
        "search": cmd_search,
        "change-state": cmd_change_state,
        "add": cmd_add,
        "edit-title": cmd_edit_title,
        "edit-description": cmd_edit_description,
        "block": cmd_block,
        "unblock": cmd_unblock,
        "relate": cmd_relate,
        "unrelate": cmd_unrelate,
        "fix": cmd_fix,
    }

    handler = commands.get(args.command)
    if handler:
        handler(args)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
