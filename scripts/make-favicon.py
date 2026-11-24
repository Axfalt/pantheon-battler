"""The site icon: a sword crossed by a lightning bolt, 32×32 pixel art.
web/img/favicon.png (32×32, the browser tab) and web/img/icon-192.png (×6, phones' home screen)."""
from PIL import Image

OUT = "E:/code/pantheon-battler/web/img/"
N = 32
hx = lambda s: tuple(int(s[i:i + 2], 16) for i in (1, 3, 5)) + (255,)
INK = hx("#2b0d16")
STEEL_DARK, STEEL, STEEL_LIGHT, SHINE = hx("#757372"), hx("#a9adb4"), hx("#d8dce2"), hx("#ffffff")
GOLD_DARK, GOLD, GOLD_LIGHT = hx("#b96c33"), hx("#de9d3d"), hx("#f3d867")
GRIP, GRIP_LIGHT = hx("#6d473d"), hx("#8c6242")
BOLT_EDGE, BOLT, BOLT_CORE = hx("#de9d3d"), hx("#f3d867"), hx("#fffbe0")


def layer():
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    px = img.load()

    def put(x, y, c):
        if 0 <= x < N and 0 <= y < N:
            px[x, y] = c

    def empty(x, y):
        return 0 <= x < N and 0 <= y < N and px[x, y][3] == 0

    return img, put, empty


def outlined(img):
    """Ink around every opaque pixel of a layer (4-neighbourhood)."""
    out = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    src, px = img.load(), out.load()
    for y in range(N):
        for x in range(N):
            if src[x, y][3]:
                continue
            if any(0 <= x + a < N and 0 <= y + b < N and src[x + a, y + b][3]
                   for a, b in ((1, 0), (-1, 0), (0, 1), (0, -1))):
                px[x, y] = INK
    out.alpha_composite(img)
    return out


def sword():
    img, put, _ = layer()
    # Blade: from the guard up to the tip (top right), 3 px wide along the diagonal.
    for k in range(18):
        x, y = 10 + k, 21 - k
        put(x, y, STEEL)            # middle
        put(x - 1, y, STEEL_LIGHT)  # lit edge
        put(x, y + 1, STEEL_DARK)   # shadow edge
        if k % 5 == 2:
            put(x - 1, y, SHINE)
    put(27, 3, STEEL_LIGHT)
    put(28, 3, STEEL_LIGHT)
    put(28, 4, STEEL)
    # Crossguard, across the blade, 2 px thick.
    for k in range(-5, 6):
        x, y = 9 + k, 21 + k
        put(x, y, GOLD_LIGHT if k < 0 else GOLD)
        put(x + 1, y, GOLD if k < 0 else GOLD_DARK)
    # Grip and pommel.
    for k in range(1, 5):
        put(9 - k, 22 + k, GRIP_LIGHT if k % 2 else GRIP)
        put(10 - k, 22 + k, GRIP)
    for x, y in ((3, 27), (4, 27), (3, 28), (4, 28), (5, 27), (4, 26)):
        put(x, y, GOLD)
    put(3, 27, GOLD_LIGHT)
    return img


def bolt():
    img, put, empty = layer()
    # A jagged bolt from the top left to the bottom right: three strokes down-right joined by
    # sharp jogs back to the left; gold edge, yellow body, white-hot core, thinning to a point.
    path = [(5, 1), (14, 10), (11, 12), (21, 21), (18, 23), (29, 30)]
    pts = []
    for (x0, y0), (x1, y1) in zip(path, path[1:]):
        n = max(abs(x1 - x0), abs(y1 - y0))
        pts += [(round(x0 + (x1 - x0) * t / n), round(y0 + (y1 - y0) * t / n)) for t in range(n)]
    pts.append(path[-1])
    tail = len(pts) - 6
    for i, (x, y) in enumerate(pts):              # body: 3×3 brush, 1 px at the tail
        r = 0 if i >= tail else 1
        for a in range(-r, r + 1):
            for b in range(-r, r + 1):
                if empty(x + a, y + b):
                    put(x + a, y + b, BOLT_EDGE)
    for i, (x, y) in enumerate(pts):              # yellow body
        put(x, y, BOLT)
        if i < tail:
            put(x + 1, y, BOLT)
            put(x, y - 1, BOLT)
    for i, (x, y) in enumerate(pts):              # white-hot core
        if 1 <= i < tail - 2:
            put(x, y, BOLT_CORE)
    return img


icon = outlined(sword())
icon.alpha_composite(outlined(bolt()))
icon.save(OUT + "favicon.png")
icon.resize((N * 6, N * 6), Image.NEAREST).save(OUT + "icon-192.png")
icon.resize((N * 10, N * 10), Image.NEAREST).save("E:/code/pantheon-battler/target/web_work/shots/favicon_x10.png")
print("ok")
