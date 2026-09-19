# 16. Image & Video Processing

A photograph is a signal like any other in this course, carried on two axes instead of one. A grayscale image is a value — brightness — that varies across row and column; a colour image stacks three of those. Nothing about convolution, filtering, or frequency content required that the independent variable be time.

## The idea

**Two-dimensional convolution.** An LTI system is a moving weighted sum of a signal's neighbours. A 2-D kernel \(h\) runs that same operation across both axes of an image \(x\) at once:

\[
y[r,c] = \sum_{i}\sum_{j} h[i,j]\, x[r-i,\, c-j]
\]

Every output pixel is a weighted sum of a small neighbourhood of input pixels, the weights fixed by \(h\). A kernel of nine equal, positive weights averages a 3x3 neighbourhood — the 2-D counterpart of a moving-average low-pass filter. A kernel built from the discrete Laplacian, which subtracts each pixel's neighbours from itself, is a high-pass filter: flat regions cancel to near zero, sudden intensity changes do not.

\[
\nabla^2 x[r,c] \;\approx\; x[r+1,c] + x[r-1,c] + x[r,c+1] + x[r,c-1] - 4\,x[r,c]
\]

Run over a whole image, that subtraction *is* edge detection: the same high-pass idea from earlier lessons, carrying a second subscript.

**Segmentation.** Once a feature like an edge is enhanced, three generic steps turn pixels into objects: **threshold** the image into foreground and background, **label** each connected group of foreground pixels with its own integer, and **measure** each region's area, centroid, and bounding box. This segment-then-measure pipeline is the same one that turns a microscopy slide into a cell count, or a satellite photo into a field boundary map — the underlying arithmetic does not know or care what the blobs represent.

The threshold is often chosen automatically rather than hand-picked. Nobuyuki Otsu's 1979 method treats every possible cut point as a candidate split of the image's own brightness histogram into two classes, and picks the one that minimizes the intensity variance within each class while maximizing the variance between them — no manual tuning, no assumption about the scene beyond "two classes exist." A threshold chosen this way adapts automatically to a brighter room, a dirtier lens, or a different exposure, which a fixed hand-picked cutoff cannot.

```qu
seed(1)
M = zeros(9, 9)
for r = 0 to 8
    for c = 0 to 8
        M[r, c] = 40
    end for
end for
for r = 3 to 5
    for c = 3 to 5
        M[r, c] = 220
    end for
end for
img = image_from_matrix(M)
imshow(img)
xlabel("column (px)")
ylabel("row (px)")
```

```qu
figure()
subplot(1, 2, 1)
imshow(blur(img))
xlabel("column (px)")
ylabel("row (px)")
title("blur kernel")
subplot(1, 2, 2)
imshow(edge_detect(img))
xlabel("column (px)")
ylabel("row (px)")
title("edge kernel")
```

One synthetic image, two kernels: the box-average kernel spreads the bright square's energy into its surroundings, while the Laplacian kernel keeps only its boundary. Both are the same equation above, read off with different weights.

## In Qu

Build a 40x40 field with a filled 16x16 bright square, run Qu's built-in Laplacian kernel (`edge_detect`), then segment and measure what survives, rather than eyeballing it:

```qu
w = 40
h = 40
bg = 30
fg = 220

M = zeros(h, w)
for r in 0 to h - 1
    for c in 0 to w - 1
        M[r, c] = bg
    end for
end for
for r in 12 to 27
    for c in 12 to 27
        M[r, c] = fg
    end for
end for

img = image_from_matrix(M)
edges = edge_detect(img)
imshow(edges)
xlabel("column (px)")
ylabel("row (px)")

level = otsu_threshold(edges)
mask = threshold(edges, level)
labeled = bwlabel(mask)
stats = regionprops(labeled)

square_area = 16 * 16
for i = 1 to labeled.count
    r = stats[i - 1]
    print("blob {r.label}: {r.area} px, centroid ({r.centroid_x}, {r.centroid_y})")
end for
print("filled square interior: {square_area} px")
```

```
blob 1: 60 px, centroid (19.5, 19.5)
filled square interior: 256 px
```

The source square holds 256 filled pixels; edge detection followed by Otsu's threshold kept exactly 60 of them — a single connected ring, roughly the square's perimeter (4x16 minus double-counted corners) — and discarded the 196-pixel flat interior, where \(\nabla^2 x = 0\) and nothing distinguishes one pixel from its neighbour. `bwlabel` assigned that ring one connected-component label; `regionprops` measured its centroid at \((19.5,\ 19.5)\), the exact geometric centre of the original 12-to-27 block, computed from under a quarter of the original pixel budget. For a task like locating an object, the interior was never carrying information the boundary didn't already have.

Not every task wants edges. If the goal were contrast rather than shape, the tool would be `imadjust` or `histeq` instead of `edge_detect` — both documented alongside it, both measurable the same way, by comparing `imhist` before and after. One worked example cannot carry every kernel in the library; what it can show is that they all resolve to the convolution equation above, read off differently.

## The limit of a still image

Everything above ran on one frame. Qu's image chapter reads and writes exactly two file formats, PNG and BMP — both still-frame formats — and the standard library has no `Video` type, no `load_video`, no codec, no frame iterator. Every operation in this lesson is something you would run once per frame, in a loop; Qu offers no help with anything that spans frames — tracking a blob across time, or finding what changed between one frame and the next. That gap points at the next kind of signal this course turns to: one that changes its own frequency content every 20 to 40 milliseconds, whether or not anyone is watching in real time. Lesson 17 opens a microphone.
