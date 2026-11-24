"""The wooden lever of the "⭐ Fusionnées" switch: web/img/lever-off.png (handle leaning left) and
web/img/lever-on.png (leaning right, brass knob lit), drawn 1:1 and shown ×2 by the page."""
from PIL import Image

OUT = "E:/code/pantheon-battler/web/img/"
W, H = 23, 19
hx = lambda s: tuple(int(s[i:i + 2], 16) for i in (1, 3, 5)) + (255,)
INK = hx("#2b0d16")
WOOD_DARK, WOOD, WOOD_LIGHT, WOOD_HI = hx("#6d473d"), hx("#8c6242"), hx("#b96c33"), hx("#c48437")
IRON, IRON_HI = hx("#3e3d42"), hx("#757372")
BRASS_LO, BRASS, BRASS_HI, SHINE = hx("#b96c33"), hx("#de9d3d"), hx("#f3d867"), hx("#fffbe0")


def lever(on):
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    px = img.load()

    def put(x, y, c):
        if 0 <= x < W and 0 <= y < H:
            px[x, y] = c

    # ---- wooden base: two planks with a darker joint, light top edge, iron bands at the ends
    x0, x1, y0, y1 = 1, W - 2, H - 7, H - 2
    for y in range(y0, y1 + 1):
        for x in range(x0, x1 + 1):
            c = WOOD
            if y == y0:
                c = WOOD_HI
            elif y == y0 + 1:
                c = WOOD_LIGHT
            elif y == (y0 + y1) // 2 + 1:
                c = WOOD_DARK                       # joint between the two planks
            elif (x * 7 + y * 3) % 11 == 0:
                c = WOOD_DARK                       # grain
            if x in (x0 + 2, x1 - 2):
                c = IRON_HI if y == y0 else IRON    # iron bands
            put(x, y, c)
    for x in range(x0 - 1, x1 + 2):                # outline
        put(x, y0 - 1, INK)
        put(x, y1 + 1, INK)
    for y in range(y0, y1 + 1):
        put(x0 - 1, y, INK)
        put(x1 + 1, y, INK)
    for x in range(x0, x1 + 2):                    # shadow under the base
        put(x + 1, y1 + 2, hx("#5c1020")) if False else None

    # ---- slot in the top of the base, where the handle pivots
    cx = W // 2
    for x in range(cx - 3, cx + 3):
        put(x, y0, INK)
        put(x, y0 + 1, IRON)

    # ---- handle: a 2 px wooden stick from the pivot to the knob, outlined
    pivot = (cx - 0.5, y0 + 1)
    tip = (cx - 7, 4) if not on else (cx + 6, 4)
    pts = set()
    steps = 40
    for k in range(steps + 1):
        t = k / steps
        x = pivot[0] + (tip[0] - pivot[0]) * t
        y = pivot[1] + (tip[1] - pivot[1]) * t
        pts.add((round(x), round(y)))
        pts.add((round(x) + 1, round(y)))
    for (x, y) in pts:                             # outline first
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            if (x + dx, y + dy) not in pts and y + dy < y0:
                put(x + dx, y + dy, INK)
    for (x, y) in pts:
        if y < y0:
            put(x, y, WOOD_LIGHT if (x + y) % 2 else WOOD_HI)

    # ---- brass knob (5×5, rounded), lit when on
    kx, ky = round(tip[0]), round(tip[1])
    shape = [" ### ", "#####", "#####", "#####", " ### "]
    for j, row in enumerate(shape):
        for i, ch in enumerate(row):
            if ch == "#":
                x, y = kx - 2 + i, ky - 2 + j
                c = BRASS
                if j >= 3 or i == 4:
                    c = BRASS_LO
                if j <= 1 and i <= 2:
                    c = BRASS_HI
                put(x, y, c)
    for j in range(-3, 4):                         # knob outline
        for i in range(-3, 4):
            x, y = kx + i, ky + j
            inside = 0 <= i + 2 < 5 and 0 <= j + 2 < 5 and shape[j + 2][i + 2] == "#"
            near = any(0 <= i + 2 + a < 5 and 0 <= j + 2 + b < 5 and shape[j + 2 + b][i + 2 + a] == "#"
                       for a, b in ((1, 0), (-1, 0), (0, 1), (0, -1)))
            if not inside and near:
                put(x, y, INK)
    put(kx - 1, ky - 1, SHINE)
    if on:                                         # a glint: the lever is engaged
        for x, y in ((kx + 4, ky - 3), (kx + 5, ky - 4), (kx + 4, ky - 5), (kx + 3, ky - 4)):
            put(x, y, BRASS_HI)
        put(kx + 4, ky - 4, SHINE)
    return img


for name, on in (("lever-off", False), ("lever-on", True)):
    lever(on).save(OUT + name + ".png")
print("ok")
