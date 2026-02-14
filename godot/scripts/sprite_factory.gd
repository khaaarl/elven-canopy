## Programmatic sprite generator for chibi elves and capybaras.
##
## Provides static factory methods that return ImageTexture objects built
## pixel-by-pixel using Image.create(). All sprites are deterministically
## generated from integer seeds, so the same seed always produces the same
## sprite. No external assets are needed.
##
## See also: elf_renderer.gd (consumes chibi elf textures),
## capybara_renderer.gd (consumes capybara textures).

class_name SpriteFactory


# ---------------------------------------------------------------------------
# Drawing helpers
# ---------------------------------------------------------------------------

static func _set_px(img: Image, x: int, y: int, color: Color) -> void:
	if x >= 0 and x < img.get_width() and y >= 0 and y < img.get_height():
		img.set_pixel(x, y, color)


static func _draw_circle(img: Image, cx: int, cy: int, r: int, color: Color) -> void:
	for y in range(cy - r, cy + r + 1):
		for x in range(cx - r, cx + r + 1):
			if (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r:
				_set_px(img, x, y, color)


static func _draw_ellipse(img: Image, cx: int, cy: int, rx: int, ry: int, color: Color) -> void:
	for y in range(cy - ry, cy + ry + 1):
		for x in range(cx - rx, cx + rx + 1):
			var dx := float(x - cx) / float(rx)
			var dy := float(y - cy) / float(ry)
			if dx * dx + dy * dy <= 1.0:
				_set_px(img, x, y, color)


static func _draw_rect(img: Image, x0: int, y0: int, w: int, h: int, color: Color) -> void:
	for y in range(y0, y0 + h):
		for x in range(x0, x0 + w):
			_set_px(img, x, y, color)


static func _draw_hline(img: Image, x0: int, x1: int, y: int, color: Color) -> void:
	for x in range(x0, x1 + 1):
		_set_px(img, x, y, color)


static func _draw_vline(img: Image, x: int, y0: int, y1: int, color: Color) -> void:
	for y in range(y0, y1 + 1):
		_set_px(img, x, y, color)


static func _darken(color: Color, amount: float = 0.3) -> Color:
	return Color(
		clampf(color.r - amount, 0.0, 1.0),
		clampf(color.g - amount, 0.0, 1.0),
		clampf(color.b - amount, 0.0, 1.0),
		color.a
	)


static func _lighten(color: Color, amount: float = 0.2) -> Color:
	return Color(
		clampf(color.r + amount, 0.0, 1.0),
		clampf(color.g + amount, 0.0, 1.0),
		clampf(color.b + amount, 0.0, 1.0),
		color.a
	)


# ---------------------------------------------------------------------------
# Chibi elf generation
# ---------------------------------------------------------------------------

## Color palettes used by elf_params_from_seed.
const HAIR_COLORS = [
	Color(0.95, 0.85, 0.40),  # blonde
	Color(0.85, 0.30, 0.20),  # red
	Color(0.20, 0.65, 0.30),  # forest green
	Color(0.35, 0.50, 0.90),  # blue
	Color(0.82, 0.82, 0.88),  # silver
	Color(0.50, 0.30, 0.15),  # brown
	Color(0.90, 0.50, 0.70),  # pink
]

const EYE_COLORS = [
	Color(0.30, 0.50, 0.90),  # blue
	Color(0.25, 0.70, 0.35),  # green
	Color(0.85, 0.65, 0.20),  # amber
	Color(0.60, 0.30, 0.80),  # violet
	Color(0.45, 0.30, 0.20),  # brown
]

const SKIN_TONES = [
	Color(0.93, 0.80, 0.65),  # fair
	Color(0.85, 0.70, 0.55),  # light
	Color(0.72, 0.55, 0.40),  # medium
	Color(0.55, 0.38, 0.25),  # dark
]

const HAIR_STYLES = ["straight_bangs", "side_swept", "wild"]
const ROLES = ["warrior", "mage", "archer", "healer", "bard"]

## Outfit base colors per role.
const ROLE_OUTFIT_COLORS = {
	"warrior": [Color(0.55, 0.20, 0.15), Color(0.40, 0.15, 0.10)],
	"mage":    [Color(0.25, 0.20, 0.65), Color(0.15, 0.12, 0.50)],
	"archer":  [Color(0.20, 0.50, 0.20), Color(0.15, 0.38, 0.15)],
	"healer":  [Color(0.90, 0.90, 0.85), Color(0.75, 0.75, 0.70)],
	"bard":    [Color(0.80, 0.55, 0.15), Color(0.65, 0.40, 0.10)],
}


## Build a deterministic params dictionary from an integer seed.
static func elf_params_from_seed(seed: int) -> Dictionary:
	# Simple hashing to spread bits.
	var h := absi(seed * 2654435761) # Knuth multiplicative hash (unsigned-ish)
	return {
		"hair_color": HAIR_COLORS[absi(h) % HAIR_COLORS.size()],
		"eye_color": EYE_COLORS[absi(h / 7) % EYE_COLORS.size()],
		"skin_tone": SKIN_TONES[absi(h / 31) % SKIN_TONES.size()],
		"hair_style": HAIR_STYLES[absi(h / 131) % HAIR_STYLES.size()],
		"role": ROLES[absi(h / 541) % ROLES.size()],
		"seed": seed,
	}


## Create a 48x48 chibi elf sprite. `params` should come from elf_params_from_seed.
static func create_chibi_elf(params: Dictionary) -> ImageTexture:
	var W := 48
	var H := 48
	var img := Image.create(W, H, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.0, 0.0, 0.0, 0.0))

	var skin: Color = params.get("skin_tone", SKIN_TONES[0])
	var hair: Color = params.get("hair_color", HAIR_COLORS[0])
	var eyes: Color = params.get("eye_color", EYE_COLORS[0])
	var style: String = params.get("hair_style", "straight_bangs")
	var role: String = params.get("role", "archer")

	var outfit_colors: Array = ROLE_OUTFIT_COLORS.get(role, ROLE_OUTFIT_COLORS["archer"])
	var outfit: Color = outfit_colors[0]
	var outfit_dark: Color = outfit_colors[1]

	var skin_dark := _darken(skin, 0.12)
	var hair_dark := _darken(hair, 0.15)
	var outline := Color(0.15, 0.12, 0.10, 1.0)
	var white := Color(1.0, 1.0, 1.0, 1.0)
	var black := Color(0.08, 0.06, 0.06, 1.0)
	var mouth := Color(0.75, 0.40, 0.40, 1.0)
	var boot_color := Color(0.35, 0.22, 0.12, 1.0)
	var belt_color := _darken(outfit, 0.20)

	var cx := W / 2  # 24

	# ------- 1. Hair back layer -------
	match style:
		"straight_bangs":
			# Long hair behind head
			_draw_ellipse(img, cx, 16, 13, 14, hair_dark)
		"side_swept":
			_draw_ellipse(img, cx + 2, 16, 14, 13, hair_dark)
		"wild":
			_draw_ellipse(img, cx, 15, 14, 15, hair_dark)
			# Wild tufts
			_draw_circle(img, cx - 10, 8, 4, hair_dark)
			_draw_circle(img, cx + 10, 8, 4, hair_dark)
			_draw_circle(img, cx - 7, 3, 3, hair_dark)
			_draw_circle(img, cx + 7, 3, 3, hair_dark)

	# ------- 2. Head -------
	var head_cy := 14
	var head_r := 11
	# Outline
	_draw_circle(img, cx, head_cy, head_r + 1, outline)
	# Fill
	_draw_circle(img, cx, head_cy, head_r, skin)
	# Subtle cheek blush
	_draw_ellipse(img, cx - 7, 18, 3, 2, Color(0.90, 0.60, 0.55, 0.45))
	_draw_ellipse(img, cx + 7, 18, 3, 2, Color(0.90, 0.60, 0.55, 0.45))

	# ------- 3. Pointed elf ears -------
	# Left ear
	for i in range(5):
		_set_px(img, cx - head_r - 1 - i, head_cy - 2 - i, outline)
		_set_px(img, cx - head_r - i, head_cy - 1 - i, skin)
		_set_px(img, cx - head_r - i, head_cy - i, skin)
	# Right ear
	for i in range(5):
		_set_px(img, cx + head_r + 1 + i, head_cy - 2 - i, outline)
		_set_px(img, cx + head_r + i, head_cy - 1 - i, skin)
		_set_px(img, cx + head_r + i, head_cy - i, skin)

	# ------- 4. Big anime eyes -------
	# Each eye is a 5x5 region
	var eye_y := head_cy - 1  # row 13
	var left_eye_x := cx - 6
	var right_eye_x := cx + 2

	for ex in range(5):
		for ey in range(5):
			# Outline (top and bottom rows, left and right cols)
			if ey == 0 or ey == 4 or ex == 0 or ex == 4:
				_set_px(img, left_eye_x + ex, eye_y + ey, outline)
				_set_px(img, right_eye_x + ex, eye_y + ey, outline)
			else:
				# Inner fill: iris
				_set_px(img, left_eye_x + ex, eye_y + ey, eyes)
				_set_px(img, right_eye_x + ex, eye_y + ey, eyes)

	# Pupils (2x2 in center of each eye)
	for px in range(2):
		for py in range(2):
			_set_px(img, left_eye_x + 2 + px, eye_y + 2 + py, black)
			_set_px(img, right_eye_x + 2 + px, eye_y + 2 + py, black)

	# White highlights (1px, top-left of iris)
	_set_px(img, left_eye_x + 1, eye_y + 1, white)
	_set_px(img, right_eye_x + 1, eye_y + 1, white)

	# ------- 5. Tiny mouth -------
	_draw_hline(img, cx - 1, cx + 1, head_cy + 6, mouth)

	# ------- 6. Hair front layer (bangs) -------
	match style:
		"straight_bangs":
			# Flat fringe across forehead
			_draw_rect(img, cx - 10, 3, 20, 7, hair)
			# Rounded top
			_draw_ellipse(img, cx, 5, 11, 5, hair)
			# Jagged bottom edge
			for i in range(-9, 10, 3):
				_set_px(img, cx + i, 10, hair)
				_set_px(img, cx + i + 1, 11, hair)
		"side_swept":
			# Swept to the right
			_draw_ellipse(img, cx + 1, 5, 11, 5, hair)
			# Diagonal fringe
			for i in range(10):
				_draw_hline(img, cx - 10 + i, cx + 10, 4 + i / 3, hair)
				if 4 + i / 3 > 8:
					break
			_draw_rect(img, cx - 10, 3, 22, 6, hair)
			# Side tuft
			_draw_circle(img, cx + 11, 8, 3, hair)
		"wild":
			# Spiky bangs
			_draw_ellipse(img, cx, 5, 12, 5, hair)
			# Spiky tufts pointing up
			for spike in range(-8, 9, 4):
				_draw_vline(img, cx + spike, 0, 6, hair)
				_set_px(img, cx + spike - 1, 1, hair)
				_set_px(img, cx + spike + 1, 1, hair)

	# ------- 7. Body / outfit -------
	var body_top := 25
	var body_bot := 36

	match role:
		"warrior":
			# Wider shoulders, armored look
			for y in range(body_top, body_bot + 1):
				var hw := 9 if y < body_top + 3 else 7
				_draw_hline(img, cx - hw, cx + hw, y, outfit)
			# Shoulder pads
			_draw_ellipse(img, cx - 9, body_top + 1, 3, 2, outfit_dark)
			_draw_ellipse(img, cx + 9, body_top + 1, 3, 2, outfit_dark)
		"mage":
			# Flowing robe, wider at bottom
			for y in range(body_top, body_bot + 3):
				var hw := 6 + (y - body_top) / 3
				_draw_hline(img, cx - hw, cx + hw, y, outfit)
			# Robe trim
			_draw_hline(img, cx - 10, cx + 10, body_bot + 2, _lighten(outfit, 0.3))
		"archer":
			# Fitted vest
			for y in range(body_top, body_bot + 1):
				var hw := 7
				_draw_hline(img, cx - hw, cx + hw, y, outfit)
		"healer":
			# White robe, similar to mage
			for y in range(body_top, body_bot + 2):
				var hw := 6 + (y - body_top) / 3
				_draw_hline(img, cx - hw, cx + hw, y, outfit)
			# Cross emblem
			_draw_vline(img, cx, body_top + 2, body_top + 6, Color(0.85, 0.20, 0.20))
			_draw_hline(img, cx - 2, cx + 2, body_top + 4, Color(0.85, 0.20, 0.20))
		"bard":
			# Colorful tunic
			for y in range(body_top, body_bot + 1):
				var hw := 7
				var c := outfit if (y - body_top) % 4 < 2 else _lighten(outfit, 0.15)
				_draw_hline(img, cx - hw, cx + hw, y, c)

	# ------- 8. Belt / sash -------
	_draw_hline(img, cx - 7, cx + 7, body_top + 6, belt_color)
	_draw_hline(img, cx - 7, cx + 7, body_top + 7, belt_color)
	# Belt buckle
	_set_px(img, cx, body_top + 6, _lighten(belt_color, 0.4))
	_set_px(img, cx, body_top + 7, _lighten(belt_color, 0.4))

	# ------- 9. Stubby arms + hands -------
	# Left arm
	_draw_rect(img, cx - 10, body_top + 2, 3, 7, skin)
	_draw_rect(img, cx - 11, body_top + 2, 1, 7, outline)
	# Left hand
	_draw_rect(img, cx - 10, body_top + 9, 3, 2, skin)
	# Right arm
	_draw_rect(img, cx + 8, body_top + 2, 3, 7, skin)
	_draw_rect(img, cx + 11, body_top + 2, 1, 7, outline)
	# Right hand
	_draw_rect(img, cx + 8, body_top + 9, 3, 2, skin)

	# ------- 10. Short legs + chunky boots -------
	var leg_top := body_bot + 1
	var leg_bot := 42
	var boot_top := 43

	# Left leg
	_draw_rect(img, cx - 5, leg_top, 4, leg_bot - leg_top, skin_dark)
	# Right leg
	_draw_rect(img, cx + 2, leg_top, 4, leg_bot - leg_top, skin_dark)

	# Left boot
	_draw_rect(img, cx - 6, boot_top, 6, 5, boot_color)
	# Right boot
	_draw_rect(img, cx + 1, boot_top, 6, 5, boot_color)

	# ------- 11. Accessories by role -------
	match role:
		"warrior":
			# Headband
			_draw_hline(img, cx - 10, cx + 10, 8, Color(0.80, 0.15, 0.10))
			_draw_hline(img, cx - 10, cx + 10, 9, Color(0.80, 0.15, 0.10))
		"mage":
			# Pointed cap tip
			for i in range(5):
				_draw_hline(img, cx - 4 + i, cx + 4 - i, i, _lighten(outfit, 0.1))
			# Star on cap
			_set_px(img, cx, 2, Color(1.0, 0.95, 0.30))
		"archer":
			# Quiver hint on back (visible as lines on right side)
			_draw_vline(img, cx + 10, body_top - 2, body_top + 8, Color(0.50, 0.35, 0.15))
			_draw_vline(img, cx + 11, body_top - 3, body_top + 7, Color(0.50, 0.35, 0.15))
			# Arrow tips
			_set_px(img, cx + 10, body_top - 3, Color(0.70, 0.70, 0.70))
			_set_px(img, cx + 11, body_top - 4, Color(0.70, 0.70, 0.70))
		"healer":
			# Circlet
			_draw_hline(img, cx - 8, cx + 8, 5, Color(0.90, 0.85, 0.30))
			_set_px(img, cx, 4, Color(0.30, 0.80, 0.90))  # Gem
		"bard":
			# Feathered cap
			_draw_circle(img, cx + 8, 3, 2, Color(0.85, 0.25, 0.25))
			_draw_vline(img, cx + 8, 0, 2, Color(0.85, 0.25, 0.25))
			# Jaunty angle feather
			_set_px(img, cx + 9, 0, Color(0.85, 0.25, 0.25))

	return ImageTexture.create_from_image(img)


# ---------------------------------------------------------------------------
# Capybara generation
# ---------------------------------------------------------------------------

const CAPY_BODY_COLORS = [
	Color(0.58, 0.42, 0.28),  # classic brown
	Color(0.65, 0.48, 0.32),  # golden brown
	Color(0.50, 0.36, 0.22),  # dark brown
	Color(0.68, 0.55, 0.40),  # sandy
]

const CAPY_ACCESSORIES = ["none", "flower_crown", "scarf", "bow"]


## Build deterministic capybara params from an integer seed.
static func capybara_params_from_seed(seed: int) -> Dictionary:
	var h := absi(seed * 2654435761)
	return {
		"body_color": CAPY_BODY_COLORS[absi(h) % CAPY_BODY_COLORS.size()],
		"accessory": CAPY_ACCESSORIES[absi(h / 13) % CAPY_ACCESSORIES.size()],
		"seed": seed,
	}


## Create a 40x32 capybara sprite. `params` should come from capybara_params_from_seed.
static func create_capybara(params: Dictionary) -> ImageTexture:
	var W := 40
	var H := 32
	var img := Image.create(W, H, false, Image.FORMAT_RGBA8)
	img.fill(Color(0.0, 0.0, 0.0, 0.0))

	var body_color: Color = params.get("body_color", CAPY_BODY_COLORS[0])
	var accessory: String = params.get("accessory", "none")

	var body_dark := _darken(body_color, 0.10)
	var body_light := _lighten(body_color, 0.10)
	var outline := Color(0.18, 0.14, 0.10, 1.0)
	var nose_color := Color(0.75, 0.50, 0.45, 1.0)
	var eye_color := Color(0.10, 0.08, 0.06, 1.0)
	var white := Color(1.0, 1.0, 1.0, 1.0)

	# ------- 1. Body — wide horizontal ellipse -------
	var body_cx := 20
	var body_cy := 18
	# Outline
	_draw_ellipse(img, body_cx, body_cy, 16, 10, outline)
	# Fill
	_draw_ellipse(img, body_cx, body_cy, 15, 9, body_color)
	# Belly highlight
	_draw_ellipse(img, body_cx + 1, body_cy + 2, 10, 5, body_light)

	# ------- 2. Head — smaller circle at front -------
	var head_cx := 6
	var head_cy := 11
	# Outline
	_draw_circle(img, head_cx, head_cy, 8, outline)
	# Fill
	_draw_circle(img, head_cx, head_cy, 7, body_color)
	# Lighter face area
	_draw_ellipse(img, head_cx - 1, head_cy + 1, 5, 4, body_light)

	# ------- 3. Snout + nostrils -------
	_draw_ellipse(img, 2, head_cy + 2, 3, 2, nose_color)
	# Nostrils
	_set_px(img, 1, head_cy + 2, _darken(nose_color, 0.2))
	_set_px(img, 3, head_cy + 2, _darken(nose_color, 0.2))

	# ------- 4. Small friendly eyes -------
	# Eye (2x2 dark with white highlight)
	_draw_rect(img, head_cx - 2, head_cy - 3, 2, 2, eye_color)
	_set_px(img, head_cx - 2, head_cy - 3, white)  # Highlight

	# ------- 5. Tiny rounded ears -------
	_draw_circle(img, head_cx - 3, head_cy - 6, 2, body_dark)
	_draw_circle(img, head_cx + 1, head_cy - 6, 2, body_dark)
	# Inner ear
	_set_px(img, head_cx - 3, head_cy - 6, nose_color)
	_set_px(img, head_cx + 1, head_cy - 6, nose_color)

	# ------- 6. Four stubby legs -------
	var leg_color := body_dark
	var leg_y := body_cy + 7
	# Front left
	_draw_rect(img, 9, leg_y, 4, 5, outline)
	_draw_rect(img, 10, leg_y, 2, 4, leg_color)
	# Front right
	_draw_rect(img, 15, leg_y, 4, 5, outline)
	_draw_rect(img, 16, leg_y, 2, 4, leg_color)
	# Back left
	_draw_rect(img, 25, leg_y, 4, 5, outline)
	_draw_rect(img, 26, leg_y, 2, 4, leg_color)
	# Back right
	_draw_rect(img, 31, leg_y, 4, 5, outline)
	_draw_rect(img, 32, leg_y, 2, 4, leg_color)

	# ------- 7. Tiny tail -------
	_set_px(img, 36, body_cy - 2, body_dark)
	_set_px(img, 37, body_cy - 3, body_dark)
	_set_px(img, 37, body_cy - 2, body_dark)

	# ------- 8. Accessories -------
	match accessory:
		"flower_crown":
			# Little flowers on head
			var flower_colors := [
				Color(0.95, 0.40, 0.50),
				Color(0.95, 0.85, 0.30),
				Color(0.55, 0.70, 0.95),
			]
			for i in range(3):
				var fx := head_cx - 4 + i * 3
				var fy := head_cy - 7
				_draw_circle(img, fx, fy, 1, flower_colors[i])
				_set_px(img, fx, fy, Color(1.0, 1.0, 0.60))  # Center
			# Vine connecting flowers
			_draw_hline(img, head_cx - 5, head_cx + 3, head_cy - 6, Color(0.25, 0.60, 0.20))
		"scarf":
			# Scarf around neck area
			var scarf_color := Color(0.85, 0.25, 0.25)
			_draw_hline(img, head_cx - 1, head_cx + 8, head_cy + 5, scarf_color)
			_draw_hline(img, head_cx, head_cx + 9, head_cy + 6, scarf_color)
			# Dangling end
			_draw_vline(img, head_cx + 9, head_cy + 6, head_cy + 9, scarf_color)
			_draw_vline(img, head_cx + 10, head_cy + 7, head_cy + 10, scarf_color)
		"bow":
			# Cute bow on head
			var bow_color := Color(0.90, 0.45, 0.60)
			# Left loop
			_draw_ellipse(img, head_cx - 3, head_cy - 7, 2, 2, bow_color)
			# Right loop
			_draw_ellipse(img, head_cx + 1, head_cy - 7, 2, 2, bow_color)
			# Center knot
			_set_px(img, head_cx - 1, head_cy - 7, _darken(bow_color, 0.2))

	return ImageTexture.create_from_image(img)
