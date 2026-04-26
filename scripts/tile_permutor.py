from PIL import Image
import numpy as np

def permute_pixels(img: Image, block_size: int, perm) -> Image:
    pixels = np.asarray(img)
    shape = pixels.shape
    if shape[0] != shape[1]:
        raise ValueError("I expected square image :(")
    N = shape[0]
    if N % block_size != 0:
        raise ValueError("Size of image must be divisible by block_size")
    new_image = np.zeros(shape, dtype=pixels.dtype)

    for s, t in perm.items():
        s0, s1 = s[0]*block_size, s[1]*block_size
        t0, t1 = t[0]*block_size, t[1]*block_size
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
