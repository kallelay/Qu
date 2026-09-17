% image_io.m -- image save/load round trip benchmark, MATLAB side.
%
% Same shape as image_io.qu/.py: 2000x2000 grayscale-from-random image.
% Matty has no imwrite/imread (checked src/graphics.py and builtins.py --
% only a matplotlib-style `imshow` for plotting, no image file codec at
% all) -- this scenario cannot run under Matty; noted in the README rather
% than silently skipped.
%
% Run (cd into this script's own directory first, same reasoning as
% binary_doubles.m):
%   matlab -batch "cd('benchmarks/file_io'); image_io"

W = 2000;
H = 2000;
bmp_path = 'matlab_random.bmp';
png_path = 'matlab_random.png';

rng(5);
m = uint8(round(rand(H, W) * 255));
img = repmat(m, [1 1 3]);

tic;
imwrite(img, bmp_path);
t_save_bmp = toc;
fprintf('imwrite .bmp (%dx%d) : %.4f s\n', W, H, t_save_bmp);

tic;
imwrite(img, png_path);
t_save_png = toc;
fprintf('imwrite .png (%dx%d) : %.4f s\n', W, H, t_save_png);

tic;
img2 = imread(bmp_path);
t_load_bmp = toc;
fprintf('imread .bmp (%dx%d) : %.4f s\n', W, H, t_load_bmp);

[h2, w2, ~] = size(img2);
fprintf('width=%d height=%d\n', w2, h2);
g2 = double(img2(:,:,1));
fprintf('mean pixel (round-tripped) = %.4f\n', mean(g2(:)));
fprintf('mean(original matrix)      = %.4f\n', mean(double(m(:))));
