"""Builds the Marco VR textures the Quest build embeds (crates/skate-game/src/xr/marcovr/).

Everything here is drawn from scratch: the MarcoVR logo (traced from a photo of
the shirt), the skater's shirt, tattoo and beard overlays, the ramp textures and
the in-headset help cards. The stock skater is only read for its UV layout and
rest pose, so the overlays land on the right texels; no retail pixels are copied.

    python tools/make_marcovr_assets.py --skater <assets>/private/skater.glb \
        --logo-photo <photo of the MarcoVR shirt> --fonts <folder with Poppins>
"""
import argparse
import json
import struct
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

OUT = Path(__file__).resolve().parent.parent / "crates/skate-game/src/xr/marcovr"
BLUE = (126, 200, 242)  # MarcoVR shirt logo, #7EC8F2
INK = (38, 44, 58)


# ---------------------------------------------------------------- glTF reading

class Glb:
    def __init__(self, path):
        b = Path(path).read_bytes()
        n = struct.unpack("<I", b[12:16])[0]
        self.j = json.loads(b[20 : 20 + n])
        self.b = b
        self.bin0 = 20 + n + 8

    def acc(self, i):
        a = self.j["accessors"][i]
        bv = self.j["bufferViews"][a["bufferView"]]
        n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[a["type"]]
        dt = {5126: np.float32, 5123: np.uint16, 5125: np.uint32, 5121: np.uint8}[a["componentType"]]
        o = self.bin0 + bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        out = np.frombuffer(self.b, dtype=dt, count=a["count"] * n, offset=o)
        return out.reshape(a["count"], n) if n > 1 else out

    def primitive(self, material):
        for p in self.j["meshes"][0]["primitives"]:
            if self.j["materials"][p["material"]]["name"] == material:
                at = p["attributes"]
                return (
                    self.acc(at["POSITION"]).astype(np.float64),
                    self.acc(at["NORMAL"]).astype(np.float64),
                    self.acc(at["TEXCOORD_0"]).astype(np.float64),
                    self.acc(p["indices"]).astype(np.int64).reshape(-1, 3),
                )
        raise KeyError(material)


def position_map(glb, material, size):
    """Per texel: model-space position and normal (NaN where no triangle)."""
    P, N, U, tris = glb.primitive(material)
    pos = np.full((size, size, 3), np.nan)
    nor = np.full((size, size, 3), np.nan)
    uv = U * size - 0.5
    for t in tris:
        a, b, c = uv[t]
        lo = np.floor(np.minimum(np.minimum(a, b), c)).astype(int)
        hi = np.ceil(np.maximum(np.maximum(a, b), c)).astype(int)
        lo = np.clip(lo - 1, 0, size - 1)
        hi = np.clip(hi + 1, 0, size - 1)
        xs, ys = np.meshgrid(np.arange(lo[0], hi[0] + 1), np.arange(lo[1], hi[1] + 1))
        d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
        if abs(d) < 1e-12:
            continue
        w0 = ((b[1] - c[1]) * (xs - c[0]) + (c[0] - b[0]) * (ys - c[1])) / d
        w1 = ((c[1] - a[1]) * (xs - c[0]) + (a[0] - c[0]) * (ys - c[1])) / d
        w2 = 1 - w0 - w1
        # A little slack so seams get filled.
        inside = (w0 > -0.08) & (w1 > -0.08) & (w2 > -0.08)
        if not inside.any():
            continue
        w = np.stack([w0, w1, w2], -1)[inside]
        yy, xx = ys[inside], xs[inside]
        fresh = np.isnan(pos[yy, xx, 0]) | ((w0 >= 0) & (w1 >= 0) & (w2 >= 0))[inside]
        pos[yy[fresh], xx[fresh]] = w[fresh] @ P[t]
        nor[yy[fresh], xx[fresh]] = w[fresh] @ N[t]
    return pos, nor


# ---------------------------------------------------------------- noise helpers

def value_noise(shape, cells, seed):
    rng = np.random.default_rng(seed)
    g = rng.random((cells + 1, cells + 1))
    g[-1, :] = g[0, :]
    g[:, -1] = g[:, 0]
    small = Image.fromarray((g * 255).astype(np.uint8))
    return np.asarray(small.resize(shape[::-1], Image.BICUBIC)).astype(np.float64) / 255


def fractal(shape, seed, octaves=5, base=4):
    out = np.zeros(shape)
    amp, total = 1.0, 0.0
    for o in range(octaves):
        out += amp * value_noise(shape, base * 2**o, seed + o)
        total += amp
        amp *= 0.5
    return out / total


def save(img, name):
    OUT.mkdir(parents=True, exist_ok=True)
    img.save(OUT / name, optimize=True)
    print("wrote", name, img.size)


# ---------------------------------------------------------------- logo

def icon_from_file(path):
    """The official icon file: blue line art on a light background."""
    a = np.asarray(Image.open(path).convert("RGB")).astype(float)
    blue = np.clip((a[..., 2] - a[..., 0] - 40) / 60, 0, 1)
    mask = Image.fromarray((blue * 255).astype(np.uint8))
    if mask.width < 1000:
        k = 1000 / mask.width
        mask = mask.resize((int(mask.width * k), int(mask.height * k)), Image.LANCZOS)
    return mask.crop(mask.getbbox())


def logo(photo, fonts, official=None):
    """The icon (official file, or traced from the shirt photo) + the wordmark in Poppins."""
    if official:
        return wordmark_beside(icon_from_file(official), fonts)
    im = Image.open(photo).convert("RGB")
    w, h = im.size
    crop = im.crop((int(w * 0.25), int(h * 0.55), int(w * 0.9), int(h * 0.95)))
    icon = crop.crop((100, 335, 255, 535))
    a = np.asarray(icon).astype(float)
    m = np.clip((a[..., 2] - 70) / 90, 0, 1) * np.clip((a[..., 2] - a[..., 0] + 10) / 40, 0, 1)
    big = Image.fromarray((m * 255).astype(np.uint8)).resize((icon.width * 6, icon.height * 6), Image.LANCZOS)
    big = np.asarray(big.filter(ImageFilter.GaussianBlur(3))).astype(float) / 255
    mask = Image.fromarray((np.clip((big - 0.35) / 0.2, 0, 1) * 255).astype(np.uint8))
    mask = mask.crop(mask.getbbox())
    return wordmark_beside(mask, fonts)


def wordmark_beside(mask, fonts):
    # Wordmark beside it, baseline near the icon's lower third, like the shirt.
    font = ImageFont.truetype(str(Path(fonts) / "Poppins-Bold.ttf"), int(mask.height * 0.34))
    text = "MarcoVR"
    tb = font.getbbox(text)
    tw, th = tb[2] - tb[0], tb[3] - tb[1]
    gap = int(mask.height * 0.06)
    W = mask.width + gap + tw
    H = mask.height
    canvas = Image.new("L", (W, H), 0)
    canvas.paste(mask, (0, 0))
    ImageDraw.Draw(canvas).text((mask.width + gap - tb[0], int(H * 0.40) - tb[1]), text, font=font, fill=255)
    return mask, canvas


def colored(mask, rgb):
    out = Image.new("RGBA", mask.size, rgb + (0,))
    out.putalpha(mask)
    return out


def sticker(wordmark, width=512):
    """Logo on a black rounded plate with a white rim: decals on ramps and the pedestal."""
    pad = 0.16
    scale = width * (1 - 2 * pad) / wordmark.width
    mark = wordmark.resize((int(wordmark.width * scale), int(wordmark.height * scale)), Image.LANCZOS)
    height = int(mark.height + width * 2 * pad * 0.6)
    plate = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    d = ImageDraw.Draw(plate)
    r = height // 4
    d.rounded_rectangle((0, 0, width - 1, height - 1), r, fill=(255, 255, 255, 255))
    rim = max(6, width // 60)
    d.rounded_rectangle((rim, rim, width - 1 - rim, height - 1 - rim), r - rim, fill=(12, 12, 14, 255))
    plate.alpha_composite(colored(mark, BLUE), ((width - mark.width) // 2, (height - mark.height) // 2))
    return plate


# ---------------------------------------------------------------- skater outfit

def shirt(glb, wordmark, fonts, size=1024):
    """Black tee for the stock top: logo on the chest, the site across the back."""
    pos, nor = position_map(glb, "Retail_OuterTorso", size)
    fabric = fractal((size, size), 11, octaves=4, base=16)
    weave = 0.5 + 0.5 * np.sin(np.arange(size)[None, :] * 1.6) * np.sin(np.arange(size)[:, None] * 1.6)
    v = 14 + 10 * fabric + 3 * weave
    rgb = np.stack([v, v, v * 1.08], -1)

    def stamp(img, x0, x1, y0, y1, facing):
        """Paint `img` (RGBA) where the shirt faces `facing` (+1 front, -1 back)
        inside model-space box x0..x1, y0..y1."""
        a = np.asarray(img).astype(float) / 255
        h, w = a.shape[:2]
        x, y, nz = pos[..., 0], pos[..., 1], nor[..., 2]
        sel = ~np.isnan(x) & (nz * facing > 0.2) & (x >= x0) & (x <= x1) & (y >= y0) & (y <= y1)
        # Front: viewer's right is model +x; back: viewer's right is -x.
        fu = (x[sel] - x0) / (x1 - x0)
        if facing < 0:
            fu = 1 - fu
        fv = (y1 - y[sel]) / (y1 - y0)
        px = a[np.clip((fv * (h - 1)).astype(int), 0, h - 1), np.clip((fu * (w - 1)).astype(int), 0, w - 1)]
        al = px[:, 3:4]
        rgb[sel] = rgb[sel] * (1 - al) + px[:, :3] * 255 * al

    mark = colored(wordmark, BLUE)
    lw = 0.20
    lh = lw * mark.height / mark.width
    stamp(mark, -lw / 2, lw / 2, 1.30 - lh / 2, 1.30 + lh / 2, 1)
    font = ImageFont.truetype(str(Path(fonts) / "Poppins-Bold.ttf"), 96)
    site = "vrmarco.com.br"
    tb = font.getbbox(site)
    back = Image.new("RGBA", (tb[2] - tb[0] + 8, tb[3] - tb[1] + 8), (0, 0, 0, 0))
    ImageDraw.Draw(back).text((4 - tb[0], 4 - tb[1]), site, font=font, fill=BLUE + (255,))
    bw = 0.22
    bh = bw * back.height / back.width
    stamp(back, -bw / 2, bw / 2, 1.40 - bh / 2, 1.40 + bh / 2, -1)
    return Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8), "RGB")


def tattoo_pattern(p, seed=3):
    """Line-art ink as a function of model position: domain-warped bands."""
    rng = np.random.default_rng(seed)
    k = rng.normal(size=(6, 3)) * 38
    x, y, z = p[..., 0], p[..., 1], p[..., 2]
    warp = sum(np.sin(x * k[i, 0] + y * k[i, 1] + z * k[i, 2] + i) for i in range(3)) * 0.9
    f = np.sin((y * 55 + z * 31 + np.abs(x) * 22) + warp * 2.2)
    g = np.sin((np.abs(x) * 61 - y * 23 + z * 47) + warp * 1.7)
    lines = np.exp(-(f**2) * 18) + 0.8 * np.exp(-(g**2) * 26)
    shade = np.clip(np.sin(warp * 1.3 + y * 9) * 0.5 + 0.2, 0, 1) * 0.55
    return np.clip(lines + shade, 0, 1)


def arm_overlay(glb, size=256):
    """Short black sleeves over the shoulders and chest, tattoo sleeves below."""
    pos, _ = position_map(glb, "Retail_Arm", size)
    out = np.zeros((size, size, 4))
    ok = ~np.isnan(pos[..., 0])
    p = np.where(ok[..., None], pos, 0)
    ax = np.abs(p[..., 0])
    shoulder = np.array([0.18, 1.43])
    elbow = np.array([0.372, 1.205])
    d = (elbow - shoulder) / np.linalg.norm(elbow - shoulder)
    t = (ax - shoulder[0]) * d[0] + (p[..., 1] - shoulder[1]) * d[1]
    neck = (p[..., 1] > 1.47) & (ax < 0.1)
    sleeve = ok & (t < 0.13) & ~neck
    hand = ax > 0.50
    ink = ok & ~sleeve & ~hand & (t >= 0.13)
    edge = np.clip((t - 0.13) / 0.02, 0, 1)
    fabric = 16 + 8 * fractal((size, size), 21, 3, 8)
    out[sleeve, :3] = fabric[sleeve, None]
    out[sleeve, 3] = 255
    a = tattoo_pattern(p) * 0.85 * edge
    out[ink, :3] = INK
    out[ink, 3] = a[ink] * 255
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def face_overlay(glb, size=256):
    """Fuller dark beard over the stock stubble, jaw and chin only."""
    pos, nor = position_map(glb, "Retail_Rostral", size)
    out = np.zeros((size, size, 4))
    ok = ~np.isnan(pos[..., 0])
    p = np.where(ok[..., None], pos, 0)
    x, y, z = p[..., 0], p[..., 1], p[..., 2]
    # Below the cheekbones, in front of the ears, above the neck.
    # Jaw line under the cheeks; up the middle only as a moustache.
    top = 1.598 + 0.012 * np.clip(1 - np.abs(x) / 0.035, 0, 1)
    jaw = ok & (y < top) & (y > 1.545) & (z > -0.02)
    lip = (np.abs(x) < 0.026) & (y > 1.594) & (y < 1.603) & (z > 0.08)
    fade = np.clip((top - y) / 0.012, 0, 1) * np.clip((y - 1.545) / 0.02, 0, 1) * np.clip((z + 0.02) / 0.04, 0, 1)
    hair = 0.6 + 0.4 * fractal((size, size), 31, 3, 32)
    a = np.where(jaw & ~lip, fade * hair * 0.7, 0)
    out[..., 0], out[..., 1], out[..., 2] = 24, 18, 15
    out[..., 3] = a * 255
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")



# ---------------------------------------------------------------- galaxy jersey

JERSEY_BLUE = np.array([30, 80, 220.0])
JERSEY_MAGENTA = np.array([170, 30, 140.0])


def jersey_bands(ax, y):
    """The jersey's diagonal shoulder-to-side stripes, as (colour, alpha) per
    texel from |x| and height: blue, magenta, a thin white line, blue."""
    d = ax - 0.115 - (1.46 - y) * 0.2
    rgb = np.zeros(ax.shape + (3,))
    a = np.zeros(ax.shape)
    for lo, hi, col in [(0.0, 0.03, JERSEY_BLUE), (0.034, 0.05, JERSEY_MAGENTA),
                        (0.054, 0.061, np.array([235, 235, 245.0])), (0.066, 0.1, JERSEY_BLUE)]:
        m = (d >= lo) & (d < hi)
        rgb[m] = col
        a[m] = 1.0
    return rgb, a


def chest_art(photo):
    """Neon frame with Marco pointing, as on the front of his jersey."""
    W, H = 400, 560
    art = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    g.polygon([(40, 40), (380, 140), (90, 520)], outline=(255, 60, 200, 255), width=22)
    g.polygon([(70, 90), (350, 170), (110, 480)], outline=(60, 230, 255, 255), width=12)
    glow = glow.filter(ImageFilter.GaussianBlur(10))
    art.alpha_composite(glow)
    lines = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    l = ImageDraw.Draw(lines)
    l.polygon([(40, 40), (380, 140), (90, 520)], outline=(255, 160, 230, 255), width=6)
    l.polygon([(70, 90), (350, 170), (110, 480)], outline=(190, 250, 255, 255), width=4)
    art.alpha_composite(lines)
    marco = Image.open(photo).convert("RGBA")
    marco = marco.crop(marco.getbbox())
    w = int(W * 0.8)
    marco = marco.resize((w, int(marco.height * w / marco.width)), Image.LANCZOS)
    # Neon grade: magenta on the left, cyan on the right.
    a = np.asarray(marco).astype(float)
    t = np.linspace(0, 1, a.shape[1])[None, :, None]
    tint = (1 - t) * np.array([255, 120, 220.0]) + t * np.array([120, 220, 255.0])
    a[..., :3] = a[..., :3] * 0.7 + tint * 0.3 * (a[..., :3].mean(-1, keepdims=True) / 255 + 0.3)
    marco = Image.fromarray(np.clip(a, 0, 255).astype(np.uint8), "RGBA")
    art.alpha_composite(marco.crop((0, 0, w, min(marco.height, H - 40))), ((W - w) // 2, 40))
    return art


def shirt_galaxy(glb, photo, size=1024):
    pos, nor = position_map(glb, "Retail_OuterTorso", size)
    ok = ~np.isnan(pos[..., 0])
    p = np.where(ok[..., None], pos, 0)
    x, y, nz = p[..., 0], p[..., 1], np.where(ok, nor[..., 2], 0)
    fabric = fractal((size, size), 11, octaves=4, base=16)
    v = 12 + 8 * fabric
    rgb = np.stack([v, v, v * 1.15], -1)
    band, alpha = jersey_bands(np.abs(x), y)
    rgb = rgb * (1 - alpha[..., None]) + band * alpha[..., None]
    # Blue lower body with a faint neon grid.
    low = np.clip((1.12 - y) / 0.1, 0, 1)
    grid = ((np.abs(np.sin(x * 160)) < 0.08) | (np.abs(np.sin(y * 160)) < 0.08)) * 0.35
    lower = JERSEY_BLUE * (0.75 + grid)[..., None]
    rgb = rgb * (1 - low[..., None]) + lower * low[..., None]
    # Blue collar round the neckline.
    collar = ok & (y > 1.445)
    rgb[collar] = JERSEY_BLUE * 1.1
    # Chest art on the front.
    art = np.asarray(chest_art(photo)).astype(float) / 255
    h, w = art.shape[:2]
    x0, x1, y0, y1 = -0.095, 0.125, 1.1, 1.405
    sel = ok & (nz > 0.2) & (x >= x0) & (x <= x1) & (y >= y0) & (y <= y1)
    fu = (x[sel] - x0) / (x1 - x0)
    fv = (y1 - y[sel]) / (y1 - y0)
    px = art[np.clip((fv * (h - 1)).astype(int), 0, h - 1), np.clip((fu * (w - 1)).astype(int), 0, w - 1)]
    al = px[:, 3:4]
    rgb[sel] = rgb[sel] * (1 - al) + px[:, :3] * 255 * al
    return Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8), "RGB")


def arm_overlay_galaxy(glb, size=256):
    """The black tee's overlay with the jersey's stripes on the sleeves and a
    blue collar band."""
    base = np.asarray(arm_overlay(glb, size)).astype(float)
    pos, _ = position_map(glb, "Retail_Arm", size)
    ok = ~np.isnan(pos[..., 0])
    p = np.where(ok[..., None], pos, 0)
    ax, y = np.abs(p[..., 0]), p[..., 1]
    sleeve = base[..., 3] > 250
    band, alpha = jersey_bands(ax, y)
    m = sleeve & (alpha > 0)
    base[m, :3] = band[m]
    collar = sleeve & (ax < 0.13) & (y > 1.43)
    base[collar, :3] = JERSEY_BLUE * 1.1
    # Blue cuffs at the sleeve's end.
    shoulder = np.array([0.18, 1.43])
    elbow = np.array([0.372, 1.205])
    d = (elbow - shoulder) / np.linalg.norm(elbow - shoulder)
    t = (ax - shoulder[0]) * d[0] + (y - shoulder[1]) * d[1]
    cuff = sleeve & (t > 0.105)
    base[cuff, :3] = JERSEY_BLUE
    return Image.fromarray(np.clip(base, 0, 255).astype(np.uint8), "RGBA")



# ---------------------------------------------------------------- the big screen

TV_SLIDES = 16


def tv_slides(thumbs_dir, extra_dir):
    """The channel's thumbnails for the park's big screen: one per video
    (the version with Marco when there is one), 512 x 288 JPEGs."""
    picks = []
    for game in sorted(Path(thumbs_dir).glob("[0-9][0-9]-*")):
        files = sorted(game.glob("*.jpg"))
        marco = [f for f in files if "-v3-" in f.name]
        if marco or files:
            picks.append((marco or files)[0])
    if extra_dir:
        picks += sorted(Path(extra_dir).glob("*.png"))
    picks = [f for f in picks if "WhatsApp" not in f.name][:TV_SLIDES]
    out = OUT / "tv"
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob("*.jpg"):
        old.unlink()
    for i in range(TV_SLIDES):
        im = Image.open(picks[i % len(picks)]).convert("RGB")
        w, h = im.size
        target = w * 9 / 16
        if h > target:
            top = (h - target) / 2
            im = im.crop((0, int(top), w, int(top + target)))
        im = im.resize((512, 288), Image.LANCZOS)
        im.save(out / f"{i:02d}.jpg", quality=86)
    print("wrote", TV_SLIDES, "screen slides from", len(picks), "thumbnails")


def tv_soon(fonts, wordmark):
    """The big screen before any clip is packed."""
    W, H = 1024, 576
    img = Image.new("RGBA", (W, H), (8, 10, 16, 255))
    a = np.zeros((H, W))
    yy, xx = np.mgrid[0:H, 0:W]
    glow = np.exp(-(((xx - W / 2) / 420) ** 2 + ((yy - H / 2) / 260) ** 2))
    bg = np.dstack([8 + 30 * glow, 10 + 70 * glow, 16 + 110 * glow, np.full((H, W), 255)])
    img = Image.fromarray(bg.astype(np.uint8), "RGBA")
    d = ImageDraw.Draw(img)
    mh = 170
    mark = colored(wordmark.resize((int(wordmark.width * mh / wordmark.height), mh), Image.LANCZOS), BLUE)
    img.alpha_composite(mark, ((W - mark.width) // 2, 110))
    f = lambda name, sz: ImageFont.truetype(str(Path(fonts) / name), sz)
    d.text((W // 2, 370), "VÍDEOS DO CANAL", font=f("Poppins-BlackItalic.ttf", 64), fill=(255, 255, 255, 255), anchor="mm")
    d.text((W // 2, 440), "EM BREVE", font=f("Poppins-BlackItalic.ttf", 48), fill=(255, 212, 0, 255), anchor="mm")
    return img


def tv_header(fonts, wordmark):
    """The strip over the screen: logo, channel name and a play button."""
    W, H = 1024, 150
    img = Image.new("RGBA", (W, H), (10, 12, 18, 255))
    d = ImageDraw.Draw(img)
    d.rectangle((0, H - 6, W, H), fill=BLUE + (255,))
    mh = 96
    mark = colored(wordmark.resize((int(wordmark.width * mh / wordmark.height), mh), Image.LANCZOS), BLUE)
    icon = mark.crop((0, 0, int(mh * 0.85), mh))
    img.alpha_composite(icon, (26, (H - mh) // 2))
    f = lambda name, sz: ImageFont.truetype(str(Path(fonts) / name), sz)
    d.text((26 + icon.width + 18, 40), "VR TUTORIAIS MARCO PEREIRA", font=f("Poppins-BlackItalic.ttf", 44), fill=(255, 255, 255, 255), anchor="lm")
    d.text((26 + icon.width + 20, 98), "youtube.com/@VrTutoriaisMarcoPereira", font=f("Poppins-Bold.ttf", 28), fill=(170, 180, 195, 255), anchor="lm")
    # YouTube-style red play button.
    bx, by, bw, bh = W - 150, 38, 120, 78
    d.rounded_rectangle((bx, by, bx + bw, by + bh), 22, fill=(255, 0, 0, 255))
    d.polygon([(bx + 48, by + 22), (bx + 48, by + bh - 22), (bx + 82, by + bh / 2)], fill=(255, 255, 255, 255))
    return img

# ---------------------------------------------------------------- ramp textures

def plywood(size=512):
    """Smooth skatepark ramp surface (skatelite / masonite): warm brown, a
    soft mottle, faint panel seams every half texture and a few screw heads."""
    y = np.arange(size)[:, None]
    x = np.arange(size)[None, :]
    mottle = fractal((size, size), 41, 5, 3)
    fine = fractal((size, size), 43, 4, 32)
    base = np.array([224, 142, 82], float)  # skatelite, #e08e52
    shade = 0.92 + 0.12 * (mottle - 0.5) + 0.04 * (fine - 0.5)
    img = base * shade[..., None]
    half = size // 2
    seam = (np.minimum(x % half, half - x % half) < 1) | (np.minimum(y % half, half - y % half) < 1)
    img[np.broadcast_to(seam, (size, size))] *= 0.8
    for sx in range(half // 4, size, half // 2):
        for sy in (8, half - 8, half + 8, size - 8):
            img[((x - sx) ** 2 + (y - sy) ** 2) < 5] *= 0.7
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), "RGB")


def concrete(size=512):
    """Smooth light skatepark concrete with faint trowel marks and joints."""
    n = fractal((size, size), 51, 6, 4)
    trowel = fractal((size, size), 53, 3, 2)
    speck = np.random.default_rng(52).random((size, size))
    v = 234 + 14 * (n - 0.5) + 8 * (trowel - 0.5) - 14 * (speck > 0.992)  # #eceae4
    x = np.arange(size)
    joint = x % size < 2
    v[:, joint] *= 0.82
    v[joint, :] *= 0.82
    img = np.stack([v, v * 0.995, v * 0.975], -1)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), "RGB")


def metal(size=256):
    rng = np.random.default_rng(61)
    streak = np.asarray(Image.fromarray((rng.random((size, 8)) * 255).astype(np.uint8)).resize((size, size), Image.BICUBIC)).astype(float) / 255
    n = fractal((size, size), 62, 4, 4)
    v = 175 + 45 * (streak - 0.5) + 25 * (n - 0.5)
    img = np.stack([v, v * 1.01, v * 1.04], -1)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), "RGB")


def marble(size=512):
    n = fractal((size, size), 71, 6, 3)
    x = np.arange(size)[None, :]
    y = np.arange(size)[:, None]
    veins = np.abs(np.sin((x * 0.02 + y * 0.012) + n * 9))
    v = 228 - 70 * np.exp(-veins * 9) - 18 * n
    img = np.stack([v, v * 0.985, v * 0.97], -1)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8), "RGB")


# ---------------------------------------------------------------- help cards

def card(fonts, wordmark, title, rows, footer, size=(1024, 640)):
    W, H = size
    img = Image.new("RGBA", size, (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, W - 1, H - 1), 44, fill=(12, 14, 20, 236))
    d.rounded_rectangle((6, 6, W - 7, H - 7), 40, outline=BLUE + (255,), width=4)
    f = lambda name, s: ImageFont.truetype(str(Path(fonts) / name), s)
    head = f("Poppins-BlackItalic.ttf", 52)
    key = f("Poppins-Bold.ttf", 30)
    body = f("Poppins-Bold.ttf", 30)
    small = f("Poppins-Bold.ttf", 24)
    mh = 70
    mark = colored(wordmark.resize((int(wordmark.width * mh / wordmark.height), mh), Image.LANCZOS), BLUE)
    img.alpha_composite(mark, (W - mark.width - 40, 34))
    d.text((44, 30), title, font=head, fill=(255, 255, 255, 255))
    y = 128
    step = (H - 128 - 76) / max(len(rows), 1)
    for k, text in rows:
        tb = d.textbbox((0, 0), k, font=key)
        kw = tb[2] - tb[0] + 30
        d.rounded_rectangle((44, y + 2, 44 + kw, y + 46), 14, fill=BLUE + (255,))
        d.text((44 + 15 - tb[0], y + 24 - (tb[1] + tb[3]) / 2), k, font=key, fill=(10, 14, 22, 255))
        d.text((44 + kw + 22, y + 24), text, font=body, fill=(240, 244, 250, 255), anchor="lm")
        y += step
    d.text((W // 2, H - 40), footer, font=small, fill=(255, 212, 0, 255), anchor="mm")
    return img


SKATE_ROWS = [
    ("ANALÓG. ESQ.", "Frente: remar sozinho  ·  lados: virar"),
    ("A", "Remar (aperte várias vezes)"),
    ("ANALÓG. DIR.", "Puxe e solte para cima = ollie · gire = flip"),
    ("GATILHOS NO AR", "Grab (segurar o skate)"),
    ("Y", "Descer / subir no skate"),
    ("X (A PÉ)", "Pular e escalar"),
    ("MENU", "Toque: menu  ·  segure 1 s: construir"),
]
# Shown on the score board's card too.
BUILD_ROWS = [
    ("ESQ. LADOS", "Escolher a peça (rampas, estátua...)"),
    ("ESQ. CIMA/BAIXO", "Aumentar / diminuir o mundo"),
    ("CLIQUE ANALÓG. ESQ.", "Pista do Marco onde aponta"),
    ("GATILHO DIR. / ESQ.", "Colocar a peça  ·  desfazer"),
    ("ANALÓG. DIR.", "Girar (lados) · tamanho (cima/baixo)"),
    ("GRIP DIR. / B", "Mover a peça  ·  apagar a peça"),
    ("SEGURAR X", "Apagar todas as peças"),
]

# Menu panel layout, pixels; xr/menu.rs mirrors these numbers.
MENU_SIZE = (1024, 1280)
MENU_ROW_TOP = 215
MENU_ROW_H = 66
MENU_ITEMS = [
    "Jogar",
    "Como jogar",
    "Pista do Marco na minha frente",
    "Mapa da pista        <   >",
    "Pegar e mover a pista",
    "Tamanho da pista        -   /   +",
    "Barreira em volta da pista",
    "Tirar a pista",
    "Apagar todas as peças",
    "Roupa do Marco",
    "Placar na minha frente",
    "Modo construção",
    "Mostrar superfícies da sala",
    "Escanear a sala de novo",
    "Online: jogar com amigos",
]


def menu_panel(fonts, wordmark):
    W, H = MENU_SIZE
    img = Image.new("RGBA", MENU_SIZE, (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, W - 1, H - 1), 56, fill=(12, 14, 20, 240))
    d.rounded_rectangle((6, 6, W - 7, H - 7), 52, outline=BLUE + (255,), width=5)
    f = lambda name, s: ImageFont.truetype(str(Path(fonts) / name), s)
    mh = 120
    mark = colored(wordmark.resize((int(wordmark.width * mh / wordmark.height), mh), Image.LANCZOS), BLUE)
    img.alpha_composite(mark, ((W - mark.width) // 2, 50))
    d.line((80, 205, W - 80, 205), fill=BLUE + (120,), width=2)
    item = f("Poppins-Bold.ttf", 32)
    for i, text in enumerate(MENU_ITEMS):
        cy = MENU_ROW_TOP + i * MENU_ROW_H + MENU_ROW_H // 2
        d.text((110, cy), text, font=item, fill=(240, 244, 250, 255), anchor="lm")
    small = f("Poppins-Bold.ttf", 28)
    d.text((W // 2, H - 56), "Analógico: escolher  ·  A: confirmar  ·  B ou MENU: fechar",
           font=small, fill=(255, 212, 0, 255), anchor="mm")
    return img


def menu_select():
    img = Image.new("RGBA", (944, 60), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((2, 2, 941, 57), 22, fill=BLUE + (60,), outline=BLUE + (255,), width=4)
    return img


# Glyph atlas for text built at runtime (xr/marcovr/font.json describes it).
FONT_FIRST, FONT_COUNT, FONT_COLS = 32, 95, 16
FONT_CELL = (64, 88)


def font_atlas(fonts):
    font = ImageFont.truetype(str(Path(fonts) / "Poppins-Bold.ttf"), 56)
    cw, ch = FONT_CELL
    rows = (FONT_COUNT + FONT_COLS - 1) // FONT_COLS
    img = Image.new("RGBA", (cw * FONT_COLS, ch * rows), (255, 255, 255, 0))
    d = ImageDraw.Draw(img)
    advance = []
    for i in range(FONT_COUNT):
        c = chr(FONT_FIRST + i)
        x, y = (i % FONT_COLS) * cw, (i // FONT_COLS) * ch
        d.text((x + 5, y + 66), c, font=font, fill=(255, 255, 255, 255), anchor="ls",
               stroke_width=4, stroke_fill=(0, 0, 0, 255))
        advance.append(round(font.getlength(c)) + 2)
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "font.json").write_text(json.dumps({
        "first": FONT_FIRST, "cols": FONT_COLS, "cell": list(FONT_CELL), "pad": 5, "advance": advance,
    }))
    return img


def hint(fonts, text):
    img = Image.new("RGBA", (1024, 150), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, 1023, 149), 40, fill=(12, 14, 20, 230), outline=BLUE + (255,), width=5)
    d.text((512, 75), text, font=ImageFont.truetype(str(Path(fonts) / "Poppins-Bold.ttf"), 40),
           fill=(255, 255, 255, 255), anchor="mm")
    return img


def arc_text(img, text, font, centre, radius, mid_angle, fill, top=True):
    """Letters along a circle: over the top (reading left to right) or along
    the bottom (also left to right, letters upright)."""
    widths = [font.getlength(c) for c in text]
    total = sum(widths) / radius  # radians
    a = mid_angle - total / 2 if top else mid_angle + total / 2
    for c, w in zip(text, widths):
        step = w / radius
        theta = a + step / 2 if top else a - step / 2
        x = centre[0] + radius * np.cos(theta)
        y = centre[1] + radius * np.sin(theta)
        tile = Image.new("RGBA", (int(w) + 40, font.size + 40), (0, 0, 0, 0))
        ImageDraw.Draw(tile).text((tile.width / 2, tile.height / 2), c, font=font, fill=fill, anchor="mm")
        rot = -np.degrees(theta) - 90 if top else -np.degrees(theta) + 90
        tile = tile.rotate(rot, resample=Image.BICUBIC, expand=True)
        img.alpha_composite(tile, (int(x - tile.width / 2), int(y - tile.height / 2)))
        a = a + step if top else a - step


def vip_coin(fonts, photo, size=512):
    """The VR Tutoriais Marco Pereira VIP group medal: gold disc, the channel
    name round the rim, Marco in his Quest 3 in the middle, a VIP banner."""
    S = size * 2  # draw large, shrink at the end
    c = (S / 2, S / 2)
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    yy, xx = np.mgrid[0:S, 0:S]
    r = np.hypot(xx - c[0], yy - c[1]) / (S / 2)
    shade = np.clip(1.0 - 0.35 * (yy / S) + 0.12 * np.cos((xx + yy) / S * 6), 0.55, 1.1)
    gold = np.stack([250 * shade, 196 * shade, 60 * shade], -1)
    dark = np.array([150, 95, 25.0])
    ring = (r > 0.97) | ((r > 0.64) & (r < 0.67))
    col = np.where(ring[..., None], dark, gold)
    alpha = (r <= 1.0) * 255
    img = Image.fromarray(np.dstack([np.clip(col, 0, 255), alpha]).astype(np.uint8), "RGBA")
    # Marco, cropped to the inner disc.
    marco = Image.open(photo).convert("RGBA")
    bb = marco.getbbox()
    marco = marco.crop(bb)
    inner = int(S * 0.62)
    w = int(inner * 0.72)
    marco = marco.resize((w, int(marco.height * w / marco.width)), Image.LANCZOS)
    face = Image.new("RGBA", (inner, inner), (0, 0, 0, 0))
    face.alpha_composite(marco, ((inner - w) // 2, int(inner * 0.09)))
    mask = Image.new("L", (inner, inner), 0)
    ImageDraw.Draw(mask).ellipse((0, 0, inner - 1, inner - 1), fill=255)
    face.putalpha(Image.fromarray(np.minimum(np.asarray(face.getchannel("A")), np.asarray(mask))))
    img.alpha_composite(face, (int(c[0] - inner / 2), int(c[1] - inner / 2)))
    f = lambda name, sz: ImageFont.truetype(str(Path(fonts) / name), sz)
    brown = (110, 66, 18, 255)
    rim = f("Poppins-Bold.ttf", int(S * 0.072))
    arc_text(img, "VR TUTORIAIS MARCO PEREIRA", rim, c, S * 0.405, -np.pi / 2, brown, top=True)
    arc_text(img, "VR TUTORIAIS MARCO PEREIRA", rim, c, S * 0.43, np.pi / 2, brown, top=False)
    arc_text(img, "GRUPO", f("Poppins-Bold.ttf", int(S * 0.05)), c, S * 0.27, -np.pi / 2, brown, top=True)
    d = ImageDraw.Draw(img)
    # Banner and stars.
    by = c[1] + S * 0.08
    d.rectangle((c[0] - S * 0.31, by, c[0] + S * 0.31, by + S * 0.15), fill=(95, 70, 40, 255))
    d.text((c[0], by + S * 0.075), "VIP", font=f("Poppins-Black.ttf", int(S * 0.15)), fill=(255, 222, 120, 255),
           anchor="mm", stroke_width=4, stroke_fill=(70, 45, 15, 255))
    for sx in (-1, 1):
        x0, y0, k = c[0] + sx * S * 0.235, c[1] + S * 0.0, S * 0.035
        pts = [(x0 + k * np.cos(np.pi / 2 + i * np.pi / 5) * (1 if i % 2 == 0 else 0.45),
                y0 - k * np.sin(np.pi / 2 + i * np.pi / 5) * (1 if i % 2 == 0 else 0.45)) for i in range(10)]
        d.polygon(pts, fill=(80, 55, 25, 255))
    return img.resize((size, size), Image.LANCZOS)


def score_board(wordmark):
    W, H = 1024, 448
    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, W - 1, H - 1), 40, fill=(10, 12, 18, 225))
    d.rounded_rectangle((5, 5, W - 6, H - 6), 36, outline=BLUE + (255,), width=5)
    mh = 64
    mark = colored(wordmark.resize((int(wordmark.width * mh / wordmark.height), mh), Image.LANCZOS), BLUE)
    img.alpha_composite(mark, ((W - mark.width) // 2, 22))
    d.line((60, 104, W - 60, 104), fill=BLUE + (110,), width=2)
    return img


def badge(fonts, text, fill, width=220):
    img = Image.new("RGBA", (width, 52), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle((0, 0, width - 1, 51), 26, fill=fill + (255,))
    d.text((width // 2, 26), text, font=ImageFont.truetype(str(Path(fonts) / "Poppins-Bold.ttf"), 26),
           fill=(10, 14, 22, 255), anchor="mm")
    return img


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--skater", required=True)
    ap.add_argument("--logo-photo", help="photo of the MarcoVR shirt (icon traced from it)")
    ap.add_argument("--logo", help="official MarcoVR icon (blue on light); wins over --logo-photo")
    ap.add_argument("--fonts", required=True)
    ap.add_argument("--marco-photo", help="cut-out of Marco in his Quest 3, for the VIP coin")
    ap.add_argument("--jersey-photo", help="cut-out of Marco pointing at the camera, for the galaxy jersey")
    ap.add_argument("--thumbs", help="the channel's thumbnail folders (NN-game/*.jpg), for the big screen")
    ap.add_argument("--more-thumbs", help="a folder of more channel thumbnails (*.png)")
    args = ap.parse_args()
    glb = Glb(args.skater)
    icon, wordmark = logo(args.logo_photo, args.fonts, args.logo)
    small = icon.resize((int(icon.width * 256 / icon.height), 256), Image.LANCZOS)
    save(colored(small, BLUE), "logo_icon.png")
    save(sticker(wordmark), "logo_sticker.png")
    save(shirt(glb, wordmark, args.fonts), "shirt_black.png")
    save(arm_overlay(glb), "arm_black.png")
    if args.jersey_photo:
        save(shirt_galaxy(glb, args.jersey_photo), "shirt_galaxy.png")
        save(arm_overlay_galaxy(glb), "arm_galaxy.png")
    save(face_overlay(glb), "face_overlay.png")
    save(plywood(), "plywood.png")
    save(concrete(), "concrete.png")
    save(metal(), "metal.png")
    save(marble(), "marble.png")
    save(card(args.fonts, wordmark, "COMO JOGAR", SKATE_ROWS,
              "A: continuar  ·  analógico para o lado: modo construção"), "help_skate.png")
    save(card(args.fonts, wordmark, "MODO CONSTRUÇÃO", BUILD_ROWS,
              "Segure MENU 1 s para entrar e sair"), "help_build.png")
    save(menu_panel(args.fonts, wordmark), "menu_panel.png")
    save(menu_select(), "menu_select.png")
    save(badge(args.fonts, "LIGADA", (120, 220, 140)), "badge_on.png")
    save(badge(args.fonts, "DESLIGADA", (150, 155, 165)), "badge_off.png")
    for i, name in enumerate(["FLOW", "PLAZA", "ARENA 360", "PRO SLS", "BOWL"]):
        save(badge(args.fonts, name, (126, 200, 242), 240), f"badge_map{i}.png")
    save(badge(args.fonts, "GALÁXIA", (200, 120, 230), 240), "badge_skin0.png")
    save(badge(args.fonts, "PRETA", (150, 155, 165), 240), "badge_skin1.png")
    save(font_atlas(args.fonts), "font.png")
    save(score_board(wordmark), "score_board.png")
    save(tv_header(args.fonts, wordmark), "tv_header.png")
    save(tv_soon(args.fonts, wordmark), "tv_soon.png")
    if args.thumbs:
        tv_slides(args.thumbs, args.more_thumbs)
    if args.marco_photo:
        save(vip_coin(args.fonts, args.marco_photo), "vip_coin.png")
    save(hint(args.fonts, "Segure o GRIP direito e leve a pista  ·  A: pronto"), "move_hint.png")


if __name__ == "__main__":
    main()
