"""Theme assets for the fight page: one frame and one seamless 64x64 background tile per pantheon.

Frames come from the avatar frames (banner removed): the Egyptian one with its ruby, the Gaulish
one with its moss, and the shared ring recoloured for Rome (wine) and the North (stone and frost).
Tiles use the footer scripts' method: value noise (here periodic, so the tile repeats seamlessly)
quantised to a small palette with 4x4 Bayer dithering."""
import math
import random
from PIL import Image

ART = "C:/Users/axfal/PixelArt/"
OUT = "E:/code/pantheon-battler/web/img/"
BANNER = (12, 3, 27, 39)   # x0, y0, x1, y1 of the banner in every 100x100 frame
DEPTH = 15                 # frame drawn with a 15 px slice (the ruby reaches x = 14)


def frame_from(path):
    im = Image.open(ART + path).convert("RGBA")
    px = im.load()
    out = Image.new("RGBA", im.size, (0, 0, 0, 0))
    q = out.load()
    W, H = im.size
    for x in range(W):
        for y in range(H):
            in_banner = BANNER[0] <= x < BANNER[2] and BANNER[1] <= y < BANNER[3]
            depth = min(x, y, W - 1 - x, H - 1 - y)
            if not in_banner and depth < DEPTH:
                q[x, y] = px[x, y]
    return out


def recolour(im, mapping):
    px = im.load()
    for x in range(im.width):
        for y in range(im.height):
            c = px[x, y]
            if c[3] and c[:3] in mapping:
                px[x, y] = mapping[c[:3]] + (255,)
    return im


hx = lambda s: tuple(int(s[i:i + 2], 16) for i in (1, 3, 5))
frame_from("cadreE_papyrus_gem_still.png").save(OUT + "frame-egypte.png")
frame_from("cadreG_boar_moss_still.png").save(OUT + "frame-gaule.png")
# Shared ring colours: dark brown, mid brown, copper, bronze, gold, light gold, cream.
RING = {(109, 71, 61): "a", (140, 98, 66): "h", (185, 108, 51): "f", (196, 132, 55): "i",
        (222, 157, 61): "c", (243, 216, 103): "b", (253, 255, 230): "e"}
ROME = {"a": "#5c1020", "h": "#8a1a2a", "f": "#b96c33", "i": "#c48437", "c": "#de9d3d", "b": "#f3d867", "e": "#fdffe6"}
NORD = {"a": "#3e3d42", "h": "#626163", "f": "#6a879b", "i": "#76838c", "c": "#a0ceea", "b": "#d6f6ff", "e": "#ffffff"}
for name, pal in (("rome", ROME), ("nord", NORD)):
    recolour(frame_from("cadreR_laurel.png"), {k: hx(pal[v]) for k, v in RING.items()}).save(OUT + f"frame-{name}.png")

# ---------------------------------------------------------------- tiles
N = 64
BAYER = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]


def dith(v, x, y):
    base = math.floor(v)
    return base + (v - base > (BAYER[y % 4][x % 4] + .5) / 16)


def make_noise(seed, octaves):
    """Periodic value noise on an N x N torus: each octave's cell size divides N."""
    rnd = random.Random(seed)
    grids = [(n, [[rnd.random() for _ in range(N // n)] for _ in range(N // n)], w) for n, w in octaves]

    def noise(x, y):
        v = 0
        for n, g, w in grids:
            k = N // n
            gx, gy = (x % N) / n, (y % N) / n
            ix, iy = int(gx), int(gy)
            fx, fy = gx - ix, gy - iy
            fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
            a, b = g[iy % k][ix % k], g[iy % k][(ix + 1) % k]
            c, d = g[(iy + 1) % k][ix % k], g[(iy + 1) % k][(ix + 1) % k]
            v += w * ((a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy)
        return v
    return noise


def tile(fn):
    im = Image.new("RGB", (N, N))
    for y in range(N):
        for x in range(N):
            im.putpixel((x, y), fn(x, y))
    return im


def pick(pal, v, x, y):
    return pal[max(0, min(len(pal) - 1, dith(v * (len(pal) - 1), x, y)))]


TAU = 2 * math.pi

# Egypt: sandstone blocks (32x16, rows offset by half a block), sandy grain, dark joints.
SAND = [hx(c) for c in ("#7e5636", "#966a42", "#ac8054", "#c29868", "#d4ae7c")]
n_e = make_noise(11, [(16, .5), (8, .3), (4, .2)])


def egypt(x, y):
    row = y // 16
    bx = (x + (16 if row % 2 else 0)) % 32
    if y % 16 == 15 or bx == 31:
        return hx("#5e3e2a")
    if y % 16 == 0 or bx == 0:
        return hx("#d4ae7c")   # lit top-left edge of each block
    v = .5 + (n_e(x, y) - .5) * 1.1 + .05 * ((row * 7) % 3 - 1)
    return pick(SAND, v, x, y)


# Rome: grey-white marble with two families of thin veins (periods dividing the tile).
MARBLE = [hx(c) for c in ("#b2aeb2", "#c4c0be", "#d2ceca", "#e2dfd9", "#eeece6")]
n_r = make_noise(5, [(16, .5), (8, .3), (4, .2)])


def rome(x, y):
    v = .55 + (n_r(x, y) - .5) * .8
    c = pick(MARBLE, v, x, y)
    t1 = math.sin(TAU * (x + y) / 64 * 2 + n_r(x, y) * 3.2)
    t2 = math.sin(TAU * (x - y) / 64 + n_r(x + 23, y + 41) * 4 + 1.3)
    if abs(t1) < .08 or abs(t2) < .05:
        c = hx("#a8a4a6")
    if abs(t1) < .03:
        c = hx("#8c8688")
    return c


# Gaul: grass with blades, a few flowers and pebbles.
GRASS = [hx(c) for c in ("#1e3a10", "#2e5a12", "#4a8a14", "#62a81c", "#76c323")]
n_g = make_noise(8, [(16, .45), (8, .35), (4, .2)])
rnd = random.Random(3)
SPOTS = {}
for _ in range(7):   # flowers (yellow, red) and pebbles, placed on the torus
    cx, cy, kind = rnd.randrange(N), rnd.randrange(N), rnd.choice(["#f4d91f", "#f4d91f", "#d04030", "stone"])
    pts = [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)] if kind != "stone" else [(0, 0), (1, 0), (0, 1), (1, 1), (2, 1)]
    for dx, dy in pts:
        SPOTS[((cx + dx) % N, (cy + dy) % N)] = hx("#8a8782" if kind == "stone" else kind)
    if kind != "stone":
        SPOTS[(cx, cy)] = hx("#fdf1a0")


def gaul(x, y):
    if (x, y) in SPOTS:
        return SPOTS[(x, y)]
    v = .42 + (n_g(x, y) - .5) * 1.0
    blade = math.sin(TAU * x / 64 * 9 + n_g(x, y * 3) * 6)   # short vertical streaks
    if blade > .93:
        v += .28
    elif blade < -.95:
        v -= .25
    return pick(GRASS, v, x, y)


# North: grey rock, dark cracks, a little frost in the hollows.
ROCK = [hx(c) for c in ("#3e3d42", "#504f53", "#626163", "#757372", "#8a8782")]
n_n = make_noise(21, [(32, .35), (16, .3), (8, .2), (4, .15)])


def nord(x, y):
    v = .5 + (n_n(x, y) - .5) * 1.2
    c = pick(ROCK, v, x, y)
    crack = math.sin(TAU * (x * 1 + y * 2) / 64 + n_n(x, y) * 5)
    if abs(crack) < .045:
        c = hx("#2c2b30")
    elif n_n(x + 17, y + 9) > .74:
        c = hx("#a0ceea") if (x + y) % 3 else hx("#d6f6ff")
    return c


for name, fn in (("egypte", egypt), ("rome", rome), ("gaule", gaul), ("nord", nord)):
    tile(fn).save(OUT + f"tex-{name}.png")
print("ok")
