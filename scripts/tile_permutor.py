from PIL import Image
import numpy as np

def permute_pixels(img: Image, block_size: int, perm) -> Image:
    """Apply `perm` to one or more stacked 4x4 blocks of `block_size`-px tiles.

    Input must be `4*block_size` wide. Height must be a positive multiple of
    `4*block_size`; each `4*block_size`-tall slice is permuted independently
    and stitched back together. This lets a multi-variant atlas (variants
    stacked vertically) round-trip through the same authoring permutation.
    """
    pixels = np.asarray(img)
    shape = pixels.shape
    side = 4 * block_size
    if shape[1] != side:
        raise ValueError(f"Image width must be {side}, got {shape[1]}")
    if shape[0] == 0 or shape[0] % side != 0:
        raise ValueError(f"Image height must be a positive multiple of {side}, got {shape[0]}")
    new_image = np.zeros(shape, dtype=pixels.dtype)

    num_blocks = shape[0] // side
    for b in range(num_blocks):
        base = b * side
        for s, t in perm.items():
            s0, s1 = base + s[0]*block_size, s[1]*block_size
            t0, t1 = base + t[0]*block_size, t[1]*block_size
            new_image[
                    t0:t0+block_size,
                    t1:t1+block_size
            ] = pixels[
                    s0:s0+block_size,
                    s1:s1+block_size,
            ]
    return Image.fromarray(new_image)


INVERSE = {
    (0, 0): (0, 3),
    (1, 0): (0, 0),
    (2, 0): (1, 3),
    (3, 0): (3, 0),
    (0, 1): (3, 3),
    (1, 1): (3, 2),
    (2, 1): (0, 1),
    (3, 1): (2, 0),
    (0, 2): (0, 2),
    (1, 2): (2, 3),
    (2, 2): (1, 0),
    (3, 2): (1, 1),
    (0, 3): (1, 2),
    (1, 3): (3, 1),
    (2, 3): (2, 2),
    (3, 3): (2, 1),
}
INVERSE = {
    (s[1], s[0]): (t[1], t[0]) for s, t in INVERSE.items()
}
PERMUTATION = {
    t: s for s, t in INVERSE.items()
}

if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser()
    parser.add_argument("source")
    parser.add_argument("destination")
    parser.add_argument("--inverse", action="store_true", help="Map native layout to fancy", default=False)

    args = parser.parse_args()

    img = Image.open(args.source)
    P = PERMUTATION if not args.inverse else INVERSE

    permute_pixels(img, 16, P).save(args.destination)
