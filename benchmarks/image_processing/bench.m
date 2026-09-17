% bench.m -- same CV pipeline as bench.qu/bench.py, via MATLAB's Image
% Processing Toolbox, on the SAME test_image.bmp (built once by
% generate_image.py).
%
% NOT RUN as part of this benchmark: this machine's MATLAB R2025b install
% has no Image Processing Toolbox (`ver` lists only "MATLAB" and "Parallel
% Computing Toolbox"; `exist('imbinarize')`, `exist('bwlabel')`,
% `exist('regionprops')`, `exist('imopen')`, `exist('imclose')`,
% `exist('imerode')`, `exist('imdilate')`, `exist('imwarp')`,
% `exist('imrotate')`, `exist('histeq')`, `exist('strel')`,
% `exist('affine2d')`, `exist('graythresh')` all return 0 -- only the two
% base-MATLAB image utilities `rgb2gray` and `imresize` are present).
% `license('test','Image_Toolbox')` reports 1 (the license entitlement
% exists) but the toolbox itself is not installed, so every function this
% script needs actually errors ("... requires Image Processing Toolbox").
% Kept here as a correct, ready-to-run reference for whenever the toolbox
% is installed -- do not report numbers from this file without actually
% running it.
%
% Run (once the toolbox is installed):
%   matlab -batch "run('benchmarks/image_processing/bench.m')"

img_path = 'benchmarks/image_processing/test_image.bmp';

img = imread(img_path);
[h, w, ~] = size(img);
fprintf('loaded             : %dx%d\n', w, h);

% --- stage 1: grayscale conversion --------------------------------------
tic;
gray = rgb2gray(img);
t_gray = toc;
fprintf('grayscale          : %.4f s\n', t_gray);

% --- stage 2: Otsu threshold ---------------------------------------------
tic;
level = graythresh(gray);          % normalized [0,1]
binary = imbinarize(gray, level);
t_otsu = toc;
fprintf('otsu_threshold     : %.4f s  (level=%.2f)\n', t_otsu, level * 255);

% --- stage 3: morphological opening (noise cleanup) ----------------------
tic;
se = strel('square', 5);           % radius 2 -> side 2*2+1=5, matches Qu/OpenCV
opened = imopen(binary, se);
t_open = toc;
fprintf('imopen              : %.4f s\n', t_open);

% --- stage 4: connected-component labeling + blob stats ------------------
tic;
labeled = bwlabel(opened, 8);
props = regionprops(labeled, 'Area', 'Centroid');
t_label = toc;
count = numel(props);
total_area = sum([props.Area]);
fprintf('label_blobs+stats  : %.4f s  (count=%d)\n', t_label, count);
fprintf('total_blob_area    = %d\n', total_area);

% --- stage 5: affine transform (rotate 15 deg + scale 0.75x, same size) --
tic;
tform = affine2d([0.75*cosd(15), -0.75*sind(15), 0; ...
                   0.75*sind(15),  0.75*cosd(15), 0; ...
                   0, 0, 1]);
outputView = imref2d(size(gray));
warped = imwarp(gray, tform, 'OutputView', outputView);
t_warp = toc;
fprintf('imwarp (rotate+scale): %.4f s  (%dx%d)\n', t_warp, size(warped,2), size(warped,1));

% --- stage 6: histogram equalization --------------------------------------
tic;
equalized = histeq(warped);
t_eq = toc;
fprintf('histeq             : %.4f s\n', t_eq);

total = t_gray + t_otsu + t_open + t_label + t_warp + t_eq;
fprintf('total              : %.4f s\n', total);

before_hist = imhist(warped);
after_hist = imhist(equalized);
fprintf('histogram std (before -> after): %.2f -> %.2f\n', std(double(before_hist)), std(double(after_hist)));

imwrite(equalized, 'benchmarks/image_processing/matlab_output_equalized.bmp');
imwrite(opened, 'benchmarks/image_processing/matlab_output_binary.bmp');
