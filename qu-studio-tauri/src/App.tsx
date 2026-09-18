import React, { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { CodeEditor } from '@qu/ui-components';
import { FigureViewer, readFigureTarget, setFigureTarget, insertPlotCode, buildFigureFiles } from '@qu/ui-components';
import { FileTree } from '@qu/ui-components';
import type { FileNode, RunCellPayload } from '@qu/ui-components';
import { Terminal } from '@qu/ui-components';
import { VersionHistoryPanel } from '@qu/ui-components';
import { HelpBrowser } from '@qu/ui-components';
import { VariableExplorer } from '@qu/ui-components';
import { Panel } from '@qu/ui-components';
import { ThemeToggle } from '@qu/ui-components';
import { useTheme } from '@qu/ui-components';
import { Mascot } from '@qu/ui-components';
import { AiDiffModal } from '@qu/ui-components';
import { TabBar } from '@qu/ui-components';
import { CommandPalette } from '@qu/ui-components';
import type { Command as PaletteCommand } from '@qu/ui-components';
import { useIDEStore } from '@qu/ui-components';
import type { FileTab } from '@qu/ui-components';
import { parseCells, getPrefixSource } from '@qu/ui-components';
import { classifyStat, confirmChange, hasLocalEdits } from '@qu/ui-components';
import type { DiskStat, DiskBaseline } from '@qu/ui-components';
import {
  Play, Square, Save, FolderOpen, FilePlus, Download, Upload,
  Bug, RotateCcw, Maximize2, Minimize2, ChevronRight, ChevronDown,
  X, Check, AlertCircle, Loader, BarChart3, Activity, Cpu,
  Database, Network, AudioWaveform, Binary, Settings, Search,
  GitBranch, Terminal as TerminalIcon, FileCode, Layers, Folder,
  Sparkles, Wand2, Command as CommandIcon, SlidersHorizontal, Box, ScatterChart, Radio, LayoutGrid,
  History, BookOpen
} from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { cn } from '@qu/ui-components';

/** Resolves `catalogMeta`'s icon NAMES to real lucide components. Kept here
 *  rather than in `catalogMeta.ts` so that file stays plain data (no JSX,
 *  unit-testable on its own). */
const CATEGORY_ICONS: Record<string, LucideIcon> = {
  AudioWaveform, Radio, Network, BarChart3, Layers, Binary, Activity, Cpu, Database, Sparkles, FileCode,
};
import { InteractiveModePanel } from './InteractiveModePanel';
import { LiveSerialPlotPanel } from './LiveSerialPlotPanel';
import { GuiDesignerPanel } from './GuiDesignerPanel';
import { DspWorkbenchPanel } from './DspWorkbenchPanel';
import { LlmProviderSettings } from './LlmProviderSettings';
import { groupCatalog, catalogTitle, CATEGORY_ICON, type CatalogCategory } from './catalogMeta';

/** Small self-dismissing banner naming which AI Assist backend answered the
 *  last request -- see the brief's "make the fallback visible to the user,
 *  not silent" requirement. Lives bottom-right, out of the way of the
 *  mascot (bottom area too, but the mascot manages its own position) and
 *  the editor. A plain fallback gets a short display; a FAILED-and-fell-
 *  back note gets longer, since that's the case the user actually needs to
 *  notice and maybe act on (e.g. go fix a bad key in AI Provider Settings). */
const LlmBackendToast: React.FC<{
  note: { backend: string; fellBack: boolean; note?: string };
  onDone: () => void;
  theme: 'light' | 'dark';
}> = ({ note, onDone, theme }) => {
  useEffect(() => {
    const timer = setTimeout(onDone, note.fellBack ? 6000 : 2500);
    return () => clearTimeout(timer);
  }, [note, onDone]);

  const dark = theme === 'dark';
  return (
    <div
      className={cn(
        'fixed bottom-4 right-4 z-[80] max-w-xs rounded-lg border px-3 py-2 text-xs shadow-lg',
        note.fellBack
          ? dark
            ? 'bg-amber-500/15 border-amber-500/30 text-amber-300'
            : 'bg-amber-50 border-amber-300 text-amber-800'
          : dark
          ? 'bg-[#161615] border-[#2c2c2a] text-[#898781]'
          : 'bg-white border-[#e1e0d9] text-[#898781]'
      )}
    >
      <div className="font-medium">{note.fellBack ? `Fell back to ${note.backend}` : `Answered by ${note.backend}`}</div>
      {note.note && <div className="mt-0.5">{note.note}</div>}
    </div>
  );
};


// How often the active file is re-checked against disk while the Studio
// window has focus. Slow on purpose: the check that actually matters is
// the one fired by the `focus` event (you alt-tab back from the editor
// that just changed the file), and this interval only covers the case
// where the file changes while Studio is already the foreground window --
// a build step, a `git checkout` in a side terminal. A tighter loop would
// buy nothing a human could perceive and would stat on every tick.
const DISK_POLL_MS = 3000;
// Tauri invoke
const invoke = async <T,>(command: string, args: Record<string, any> = {}): Promise<T> => {
  try {
    const { invoke: tauriInvoke } = await import('@tauri-apps/api/tauri');
    return await tauriInvoke<T>(command, args);
  } catch (error) {
    console.error('Tauri invoke error:', error);
    throw error;
  }
};

// Native "open folder" dialog. Dynamically imported (mirrors `invoke` above)
// so this module still loads fine outside a real Tauri window (e.g. a plain
// browser tab during `npm run dev`), where `window.__TAURI_IPC__` is absent.
const openFolderDialog = async (): Promise<string | null> => {
  const { open } = await import('@tauri-apps/api/dialog');
  const selection = await open({ directory: true, multiple: false });
  if (Array.isArray(selection)) return selection[0] ?? null;
  return selection ?? null;
};

// Native "save as" dialog, for the unsaved/new-file case (no `currentFilePath`
// yet) -- mirrors `openFolderDialog` above (dynamic import so this module
// still loads in a plain browser tab during `npm run dev`).
const saveFileDialog = async (): Promise<string | null> => {
  const { save } = await import('@tauri-apps/api/dialog');
  const selection = await save({
    filters: [{ name: 'Qu Script', extensions: ['qu'] }],
    defaultPath: 'script.qu',
  });
  return selection ?? null;
};

// Mirrors the Rust `FileInfo` struct in src-tauri/src/main.rs.
interface FileInfo {
  name: string;
  is_dir: boolean;
  size: number | null;
  path: string;
}

// A real, on-disk example script from `catalog/` (see `get_examples_dir`
// in src-tauri/src/main.rs) -- distinct from the hardcoded in-app
// `SIGNAL_PROCESSING_EXAMPLES`/`ML_ALGORITHMS` template lists below.
// Clicking one REPLACES the whole editor buffer, same interaction as
// those hardcoded lists.
interface CatalogEntry {
  name: string;
  path: string;
}

// A short, reusable code fragment from `catalog/snippets/` -- named and
// described by its own `# @name: ...` / `# @desc: ...` header comment
// lines (stripped before insertion). Clicking one INSERTS at the cursor
// rather than replacing the buffer, since a snippet is meant to be
// dropped into code already being written.
interface SnippetEntry {
  name: string;
  desc: string;
  path: string;
}

const SNIPPET_NAME_RE = /^#\s*@name:\s*(.*)$/;
const SNIPPET_DESC_RE = /^#\s*@desc:\s*(.*)$/;

/** Strips a snippet file's `# @name:`/`# @desc:` header lines, returning
 *  the insertable body. Falls back to the raw text if the file has no
 *  such header (so a hand-added snippet without one still works). */
function stripSnippetHeader(source: string): string {
  const lines = source.split(/\r\n|\r|\n/);
  let start = 0;
  while (start < lines.length && (SNIPPET_NAME_RE.test(lines[start]) || SNIPPET_DESC_RE.test(lines[start]))) {
    start++;
  }
  return lines.slice(start).join('\n').replace(/^\n+/, '');
}

// Directory names to skip while recursively building the file tree --
// noisy/huge and never useful to browse from the IDE.
const SKIPPED_DIR_NAMES = new Set([
  'node_modules', 'target', '.git', 'dist', '.vscode', '.idea',
]);

// FileTree (from @qu/ui-components) expects the *entire* nested tree
// up front -- it has no lazy-loading hook of its own -- so "opening a
// folder" means walking it eagerly via repeated `list_directory` calls.
// Depth is capped defensively in case of unexpectedly deep/symlinked trees.
const MAX_TREE_DEPTH = 12;

async function buildFileTree(dirPath: string, depth = 0): Promise<FileNode[]> {
  const entries = await invoke<FileInfo[]>('list_directory', { path: dirPath });

  const visible = entries.filter(e => !(e.is_dir && SKIPPED_DIR_NAMES.has(e.name)));
  visible.sort((a, b) => {
    if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

  const nodes: FileNode[] = [];
  for (const entry of visible) {
    if (entry.is_dir) {
      const children = depth < MAX_TREE_DEPTH
        ? await buildFileTree(entry.path, depth + 1).catch(() => [])
        : [];
      nodes.push({
        id: entry.path,
        name: entry.name,
        type: 'folder',
        path: entry.path,
        children,
      });
    } else {
      nodes.push({
        id: entry.path,
        name: entry.name,
        type: 'file',
        path: entry.path,
      });
    }
  }
  return nodes;
}

// ============ Types ============

interface ExecutionState {
  isRunning: boolean;
  progress: number;
  error: string | null;
  elapsed_ms?: number;
}

// Mirrors the Rust ExecuteResponse struct in src-tauri/src/main.rs.
interface ExecuteResponse {
  success: boolean;
  output: string;
  error: string | null;
  elapsed_ms: number;
  // `data:image/svg+xml;base64,...` / `data:image/png;base64,...` URIs --
  // see `collect_plot_outputs` in src-tauri/src/main.rs for what produces
  // these (the auto-rendered final figure state, plus any files the script
  // itself wrote via explicit `savefig(...)` calls).
  plots: string[];
  // The script's final top-level variable bindings, from `--emit-vars` (see
  // `read_vars_output`/`VariableInfo` in src-tauri/src/main.rs). Empty when
  // the run produced none or the emitted file couldn't be read/parsed.
  variables: Variable[];
  // The script's final `number`/`bool`/`vector`/`matrix` bindings as
  // UNTRUNCATED numeric data, from `--emit-data` (see
  // `read_data_output`/`PlotVar` in src-tauri/src/main.rs). This is what
  // Interactive Mode (`InteractiveModePanel.tsx`) actually plots -- the
  // `variables` field above is a display-only truncated preview, not real
  // data a chart could use.
  data: PlotVar[];
}

interface Variable {
  name: string;
  type: string;
  value: string;
  size?: string;
}

// Mirrors the Rust `PlotVar` struct in src-tauri/src/main.rs. `shape` is
// `[rows, cols]`; `data` is that many numbers in qu-core's own column-major
// order for a `matrix`-typed binding (irrelevant for a `vector`, where
// `shape == [len, 1]`).
export interface PlotVar {
  name: string;
  type: string;
  shape: number[];
  /** `null` where a value is infinite or NaN -- JSON can't spell either, so
   *  `qu run --emit-data` writes them as null (and Qu's built-in `inf`/`nan`
   *  globals mean every run contains some). See `PlotVar.data`'s own comment
   *  in `src-tauri/src/main.rs`: typing this as plain `number[]` on the Rust
   *  side is what silently emptied every run's data and left Interactive Mode
   *  with nothing to plot. Plotly renders null as a gap, which is the honest
   *  rendering of a non-finite sample. */
  data: (number | null)[];
}

interface Diagnostic {
  code: string;
  severity: 'error' | 'warning';
  message: string;
  line?: number;
  column?: number;
}

interface SignalProcessingTool {
  id: string;
  name: string;
  icon: React.ReactNode;
  description: string;
  template: string;
}

interface MLAlgorithm {
  id: string;
  name: string;
  category: 'supervised' | 'unsupervised' | 'neural';
  template: string;
}

// ============ Examples ============

const SIGNAL_PROCESSING_EXAMPLES: SignalProcessingTool[] = [
  {
    id: 'fft_analysis',
    name: 'FFT Analysis',
    icon: <AudioWaveform size={16} />,
    description: 'Frequency spectrum analysis',
    template: `# FFT Spectrum Analysis
Fs = 10000  # Sampling frequency
N = 4096    # Number of samples
t = 0 to (N-1)/Fs step 1/Fs

# Multi-tone signal
f1 = 50     # 50 Hz
f2 = 120    # 120 Hz
x = sin(2*pi*f1*t) + 0.5*sin(2*pi*f2*t)

# Compute FFT
X = abs(fft(x))
f = (0:length(X)-1) * Fs / N

# Plot time domain
subplot(2, 1, 1)
plot(t, x)
title('Time Domain Signal')
xlabel('Time (s)')

# Plot frequency domain
subplot(2, 1, 2)
plot(f[1:N/2], X[1:N/2])
title('Frequency Spectrum')
xlabel('Frequency (Hz)')
grid on

print "FFT peaks at: {f[where(X > max(X)*0.1)][1:5]}"`
  },
  {
    id: 'filter_design',
    name: 'Filter Design',
    icon: <Activity size={16} />,
    description: 'Butterworth lowpass filter',
    template: `# Butterworth Lowpass Filter Design
Fs = 1000    # Sampling rate
Fc = 100     # Cutoff frequency
N = 4        # Filter order

# Generate test signal (100 Hz + 300 Hz)
t = 0 to 1 step 1/Fs
x = sin(2*pi*100*t) + sin(2*pi*300*t)

# Design Butterworth filter
lp = butter(N, 'low', Fc, Fs)

# Apply filter
y = filtfilt(lp, x)

# Plot comparison
subplot(2, 1, 1)
plot(t[1:100], x[1:100])
title('Original Signal (100Hz + 300Hz)')

subplot(2, 1, 2)
plot(t[1:100], y[1:100])
title('Filtered Signal (100Hz only)')

# Frequency response
H = freqz(lp, 1024)
f = (0:1023) * (Fs/2) / 1023
figure()
plot(f, 20*log10(abs(H)))
title('Filter Frequency Response')
xlabel('Frequency (Hz)')
ylabel('Magnitude (dB)')
grid on`
  },
  {
    id: 'spectrogram',
    name: 'Spectrogram',
    icon: <Layers size={16} />,
    description: 'Time-frequency analysis',
    template: `# Spectrogram - Time-Frequency Analysis
Fs = 1000
t = 0 to 2 step 1/Fs

# Chirp signal (frequency increases over time). chirp's real signature is
# chirp(f0, f1, fs, n) -- NOT chirp(t, f0, t1, f1) -- so the sweep duration
# comes from the sample count (length(t)), not from a time vector argument.
# t[-1] (Python-style negative indexing) also isn't valid Qu; the last
# element of a vector is t[end] (or t[end-1], etc).
f0 = 10    # Start frequency
f1 = 200   # End frequency
x = chirp(f0, f1, Fs, length(t))

# Compute and plot the spectrogram. spectrogram(...) draws its own heatmap
# directly (there's no [S, f, time] = ... bracket-destructuring return --
# Qu doesn't support that syntax, and this builtin doesn't hand back S/f/time
# separately anyway), so just call it and label the axes.
window_size = 256
overlap = 128
spectrogram(x, window_size, overlap, Fs)
title('Spectrogram')
xlabel('Time (s)')
ylabel('Frequency (Hz)')

print "Spectrogram computed: {length(x)} samples, chirp {f0}-{f1} Hz"`
  },
  {
    id: 'convolution',
    name: 'Convolution',
    icon: <AudioWaveform size={16} />,
    description: 'Signal convolution & correlation',
    template: `# Convolution and Correlation
# Impulse response of a system
h = [1, 0.5, 0.25, 0.125, 0.0625]

# Input signal
n = 0 to 50
x = sin(2*pi*0.1*n) + 0.5*sin(2*pi*0.3*n)

# Convolution
y = conv(x, h)

# Plot
subplot(3, 1, 1)
stem(n, x)
title('Input Signal')

subplot(3, 1, 2)
stem(0:length(h)-1, h)
title('Impulse Response')

subplot(3, 1, 3)
stem(0:length(y)-1, y)
title('Output (Convolution)')

# Cross-correlation
r = xcorr(x, y)
figure()
plot(r)
title('Cross-Correlation')`
  }
];

const ML_ALGORITHMS: MLAlgorithm[] = [
  {
    id: 'linear_regression',
    name: 'Linear Regression',
    category: 'supervised',
    template: `# Linear Regression with Gradient Descent
# Generate synthetic data
N = 100
X = randn(N, 1) * 10
y = 2.5*X + 1.2 + randn(N, 1) * 2

# Add bias term. [ones(N,1), X] would just concatenate the two columns
# end-to-end into one length-2N vector (they're plain orientation-free
# vectors, not real (N,1) column matrices, until reshaped) -- reshape each
# explicitly first so hstack builds a genuine (N, 2) design matrix.
X_b = hstack(reshape(ones(N, 1), N, 1), reshape(X, N, 1))

# Gradient descent parameters
alpha = 0.01  # Learning rate
iterations = 1000
theta = zeros(2, 1)  # Parameters

# Cost function and gradient. Qu's mod is an infix operator ("i mod 100"),
# not a callable function -- mod(i, 100) is a parse error.
for i = 1:iterations
    predictions = X_b * theta
    errors = predictions - y
    gradient = (1/N) * X_b' * errors
    theta = theta - alpha * gradient

    if i mod 100 == 0
        cost = (1/(2*N)) * sum(errors.^2)
        print "Iteration {i}: Cost = {cost:.4f}"
    end
end

print "Final parameters: theta = {theta}"
# Qu indexing is 0-based: theta[0] is the intercept, theta[1] the slope.
print "Hypothesis: y = {theta[1]:.3f}*x + {theta[0]:.3f}"

# Plot results
scatter(X, y)
hold on
X_plot = min(X) to max(X) step 0.1
y_plot = theta[0] + theta[1] * X_plot
plot(X_plot, y_plot, 'r-', linewidth=2)
legend('Data', 'Linear Fit')`
  },
  {
    id: 'kmeans',
    name: 'K-Means Clustering',
    category: 'unsupervised',
    template: `# K-Means Clustering
# Generate clustered data
N = 300
k = 3  # Number of clusters

# Create 3 clusters. Qu's matrix-literal grammar requires [...; ...] to sit
# on one line, so a multi-line literal here is a parse error -- build the
# stacked blocks with vstack instead (each randn(100,2)+[...] block is
# already a genuine (100,2) 2-D matrix, so vstack concatenates rows cleanly).
X = vstack(randn(100, 2) + [2, 2], randn(100, 2) + [-2, -2], randn(100, 2) + [2, -2])

# K-means algorithm. randi's real signature is randi(lo, hi, rows, cols)
# (both bounds inclusive) -- not randi(range, rows, cols) -- and indices
# are 0-based, so valid row indices into X run 0..N-1.
max_iters = 50
centroids = X[randi(0, N-1, k, 1), :]
labels = zeros(N, 1)

for iter = 1:max_iters
    # Assign points to nearest centroid. min(x, [], dim) with a MATLAB-style
    # ignored-output ([~, labels] = ...) doesn't exist in Qu, and there's no
    # axis-aware argmin either -- so find each row's nearest centroid with a
    # plain per-row argmin loop (0-based row/column indices throughout).
    distances = zeros(N, k)
    for j = 0:k-1
        distances[:, j] = sqrt(sum((X - centroids[j, :]).^2, axis=1))
    end
    for i = 0:N-1
        labels[i] = argmin(distances[i, :])
    end

    # Update centroids. mean(m, dim) also only binds via the named axis=
    # keyword -- a positional second argument is silently ignored.
    new_centroids = zeros(k, 2)
    for j = 0:k-1
        cluster_points = X[where(labels == j), :]
        if size(cluster_points)[0] > 0
            new_centroids[j, :] = mean(cluster_points, axis=0)
        end
    end

    # Check convergence. Matrix == matrix comparison isn't supported yet,
    # so compare the total centroid shift instead of an elementwise mask.
    shift = sum(abs(centroids - new_centroids))
    if shift < 1e-9
        print "Converged at iteration {iter}"
        break
    end

    centroids = new_centroids
end

# Plot clusters. Qu's [...] literal is numeric-only (no string arrays), so
# rather than indexing into a colors list, just let each scatter call pick
# up the next color in the automatic per-series color cycle.
for j = 0:k-1
    cluster = X[where(labels == j), :]
    scatter(cluster[:, 0], cluster[:, 1])
    hold on
end

# Plot centroids
scatter(centroids[:, 0], centroids[:, 1], marker='x', color='k', markersize=15)
title("K-Means Clustering (k=" + k + ")")
legend('Cluster 1', 'Cluster 2', 'Cluster 3', 'Centroids')`
  },
  {
    id: 'pca',
    name: 'Principal Component Analysis',
    category: 'unsupervised',
    template: `# Principal Component Analysis (PCA)
# Generate correlated data
N = 200
X1 = randn(N, 1)
X2 = 0.8*X1 + randn(N, 1) * 0.5
# X1/X2 are plain (orientation-free) vectors here, so [X1, X2]/hstack would
# just concatenate them end-to-end into one length-400 vector. reshape each
# into an explicit (N, 1) column matrix first so hstack builds a real (N, 2)
# data matrix instead.
X = hstack(reshape(X1, N, 1), reshape(X2, N, 1))

# Mean center. mean(X, axis=0) averages down each column, giving one mean
# per feature (mean(X, 1) would silently be ignored: axis only binds to the
# named axis=/dim= keyword, not a positional argument).
X_mean = mean(X, axis=0)
X_centered = X - X_mean

# Covariance matrix (Qu's cov(x, y) is pairwise-scalar only, so build the
# (d, d) covariance matrix directly: X_centered' * X_centered / (N - 1))
C = (X_centered' * X_centered) / (N - 1)

# Eigen decomposition
ed = eig(C)
eigvec = ed.vectors
eigval = ed.values

# Sort by eigenvalue (descending). Qu indexing is 0-based throughout, and
# argsort's returned permutation is already 0-based, so it feeds straight
# into fancy indexing with no offset.
idx = argsort(eigval, true)
sorted_vals = sort(eigval, true)
sorted_vecs = eigvec[:, idx]

# Principal components
PC1 = X_centered * sorted_vecs[:, 0]
PC2 = X_centered * sorted_vecs[:, 1]

# Explained variance
total_var = sum(sorted_vals)
explained = sorted_vals / total_var * 100

print "PCA Results:"
print "  PC1 explains: {explained[0]:.2f}%"
print "  PC2 explains: {explained[1]:.2f}%"
print "  Total: {sum(explained[0:2]):.2f}%"

# Plot
subplot(1, 2, 1)
scatter(X[:, 0], X[:, 1])
title('Original Data')
xlabel('X1')
ylabel('X2')
axis equal

subplot(1, 2, 2)
scatter(PC1, PC2)
title('PCA Transformed Data')
xlabel('PC1')
ylabel('PC2')
axis equal`
  },
  {
    id: 'neural_network',
    name: 'Neural Network',
    category: 'neural',
    template: `# Simple Neural Network for Classification
# Generate XOR-quadrant dataset. Qu's matrix-literal grammar needs [...; ...]
# on one line, so build the 4 stacked blocks with vstack instead (each
# randn(N/4,2)+[...] block is already a genuine 2-D matrix, so it stacks
# cleanly); the label blocks need an explicit reshape first since a plain
# ones(n,1)/zeros(n,1) is an orientation-free vector until reshaped, and
# vstack of those otherwise just concatenates them end-to-end.
N = 200
q = N/4
X = vstack(randn(q, 2) + [1, 1], randn(q, 2) + [-1, -1], randn(q, 2) + [1, -1], randn(q, 2) + [-1, 1])
y = vstack(reshape(ones(q, 1), q, 1), reshape(ones(q, 1), q, 1), reshape(zeros(q, 1), q, 1), reshape(zeros(q, 1), q, 1))

# Neural network parameters
input_size = 2
hidden_size = 8
output_size = 1

# Initialize weights (Xavier initialization). W2 is reshaped for the same
# orientation-free-vector reason as above: with output_size=1 it would
# otherwise silently fail to transpose (W2' below).
W1 = randn(input_size, hidden_size) * sqrt(2/input_size)
b1 = zeros(1, hidden_size)
W2 = reshape(randn(hidden_size, output_size), hidden_size, output_size) * sqrt(2/hidden_size)
b2 = zeros(1, output_size)

# Sigmoid activation. Qu has no anonymous-lambda syntax ("(x) -> expr" is
# reserved for the reshape operator) -- the real one-line function form is
# "name(x) := expr".
sigmoid(x) := 1 ./ (1 + exp(-x))
sigmoid_grad(x) := x .* (1 - x)

# Training parameters
alpha = 0.5
epochs = 5000

# Forward and backward pass
for epoch = 1:epochs
    # Forward pass
    z2 = X * W1 + b1
    a2 = sigmoid(z2)
    z3 = a2 * W2 + b2
    a3 = sigmoid(z3)

    # Backward pass. For a sigmoid output with binary cross-entropy loss,
    # dL/dz3 simplifies exactly to (a3 - y) -- the extra sigmoid_grad(a3)
    # factor the MSE-style formula would need doesn't belong here, and left
    # in it makes the gradient vanish badly on saturated units, so training
    # barely moves the loss at all.
    delta3 = (a3 - y)
    delta2 = (delta3 * W2') .* sigmoid_grad(a2)

    # Gradient descent. mean(m, dim) only binds via the named axis= keyword.
    W2 = W2 - alpha * (a2' * delta3) / N
    b2 = b2 - alpha * mean(delta3, axis=0)
    W1 = W1 - alpha * (X' * delta2) / N
    b1 = b1 - alpha * mean(delta2, axis=0)

    if epoch mod 500 == 0
        loss = -mean(y .* log(a3 + 1e-15) + (1-y) .* log(1-a3 + 1e-15))
        print "Epoch {epoch}: Loss = {loss:.4f}"
    end
end

# Predictions. A matrix built through matrix multiplication doesn't support
# comparison operators directly -- pull out the lone column as a plain
# vector first, and cast the resulting true/false mask back to 0/1 numbers
# before summing/averaging it.
z2 = X * W1 + b1
a2 = sigmoid(z2)
z3 = a2 * W2 + b2
y_pred = sigmoid(z3)
pred_class = cast(y_pred[:, 0] > 0.5, "vec")
matches = cast(abs(pred_class - y) < 0.5, "vec")
accuracy = mean(matches) * 100

print "Training complete! Accuracy: {accuracy:.2f}%"

# Plot decision boundary
scatter(X[:, 0], X[:, 1], c=y)
title('Neural Network Classification')
colorbar`
  }
];

// ============ Workspace modes ============

/** The top bar's mode row, as data. Seven near-identical buttons were
 *  written out by hand here as each tab landed, and they had already
 *  drifted apart in their hover/active colours -- a list is the cheapest
 *  guarantee that the eighth one matches. ORDER IS THE INFORMATION
 *  HIERARCHY of this bar, so it is deliberate rather than chronological:
 *  Code first (the thing you open the app to do), then the three
 *  authoring surfaces that generate Qu for you (DSP, Designer,
 *  Interactive), then ML's template library, then the two
 *  data-in/data-out tabs (SeriPlot, Files). It was previously the order
 *  the tabs happened to be built in. */
type ModeId = 'code' | 'dsp' | 'ml' | 'files' | 'interactive' | 'seriplot' | 'designer';

const MODE_TABS: Array<{ id: ModeId; label: string; icon: LucideIcon }> = [
  { id: 'code', label: 'Code', icon: FileCode },
  { id: 'dsp', label: 'DSP', icon: AudioWaveform },
  { id: 'designer', label: 'Designer', icon: LayoutGrid },
  { id: 'interactive', label: 'Interactive', icon: SlidersHorizontal },
  { id: 'ml', label: 'ML', icon: Network },
  { id: 'seriplot', label: 'SeriPlot', icon: Radio },
  { id: 'files', label: 'Files', icon: Folder },
];

// ============ Main App Component ============

function App() {
  const { resolvedTheme, toggleTheme } = useTheme();

  // State
  const [activeTab, setActiveTab] = useState<ModeId>('code');
  const [showVersionHistory, setShowVersionHistory] = useState(false);
  const [showHelpBrowser, setShowHelpBrowser] = useState(false);

  // ---- Multi-tab file model (ideStore, from @qu/ui-components) ----
  // Replaces the old single `code`/`currentFilePath` useState pair. `code`/
  // `currentFilePath` below are DERIVED from the active tab, not their own
  // state -- every former `setCode`/`setCurrentFilePath` call site now goes
  // through one of the store actions (`updateFileContent`, `openFile`,
  // `markFileSaved`, `newUntitledFile`) instead, so "which buffer am I
  // editing" has exactly one source of truth.
  const tabs = useIDEStore((s) => s.tabs);
  const activeFileId = useIDEStore((s) => s.activeFileId);
  const storeOpenFile = useIDEStore((s) => s.openFile);
  const storeCloseFile = useIDEStore((s) => s.closeFile);
  const updateFileContent = useIDEStore((s) => s.updateFileContent);
  const markFileSaved = useIDEStore((s) => s.markFileSaved);
  const newUntitledFile = useIDEStore((s) => s.newUntitledFile);
  const cycleActiveFile = useIDEStore((s) => s.cycleActiveFile);
  const setActiveFileId = useIDEStore((s) => s.setActiveFile);

  const currentTab: FileTab | null = tabs.find((t) => t.id === activeFileId) ?? null;
  const code = currentTab?.content ?? '';

  // Screen vs publication is a property of the SCRIPT, not of the viewer:
  // the engine draws the figure, so the toggle reads and writes the code's
  // own `theme(...)` line. That also means the choice is saved with the
  // file and survives a re-run, which a panel-local setting would not.
  const figureTarget = useMemo(() => readFigureTarget(code), [code]);
  const setFigureTargetInCode = useCallback(
    (next: 'screen' | 'publication') => {
      if (!activeFileId) return;
      const updated = setFigureTarget(code, next);
      // `setFigureTarget` returns the input unchanged for a no-op, so this
      // avoids marking a file dirty for a click that changed nothing.
      if (updated !== code) updateFileContent(activeFileId, updated);
    },
    [activeFileId, code, updateFileContent],
  );

  // Lines the figure editor generated, inserted where they will actually
  // take effect -- see `insertPlotCode` for why the end of the file is the
  // wrong place.
  const insertFigureCode = useCallback(
    (lines: string[]) => {
      if (!activeFileId || !lines.length) return;
      updateFileContent(activeFileId, insertPlotCode(code, lines));
    },
    [activeFileId, code, updateFileContent],
  );


  // Seed the very first tab on mount (the store starts empty) -- matches
  // the old default of opening with the first signal-processing template
  // already loaded. Guarded on `tabs.length` so this is a true one-time
  // seed, not a re-seed on every render or a Fast Refresh remount that
  // finds the store already populated.
  useEffect(() => {
    if (useIDEStore.getState().tabs.length === 0) {
      const first = SIGNAL_PROCESSING_EXAMPLES[0];
      storeOpenFile({
        id: `template:${first.id}`,
        name: `${first.name}.qu`,
        path: null,
        content: first.template,
        isDirty: false,
        language: 'qu',
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Opens a file/template/catalog-entry in its own tab, or focuses the
  // existing tab for the same `id` (a real path for on-disk files, or a
  // synthetic `template:...` id for the hardcoded template lists) rather
  // than ever overwriting the current buffer -- the core fix for the
  // "clicking a template silently discards your edits" problem.
  const openOrFocusTab = useCallback((tab: { id: string; name: string; path: string | null; content: string }) => {
    storeOpenFile({ id: tab.id, name: tab.name, path: tab.path, content: tab.content, isDirty: false, language: 'qu' });
  }, [storeOpenFile]);

  // ---- Lightweight in-app confirm modal (unsaved-changes guard) ----
  // A native `window.confirm`/Tauri `dialog.confirm()` blocking dialog would
  // work too, but this stays visually consistent with the app's other
  // review surfaces (AiDiffModal) and behaves identically in and out of a
  // real Tauri window (e.g. during `npm run dev` in a plain browser tab).
  const [confirmRequest, setConfirmRequest] = useState<{
    message: string;
    confirmLabel: string;
    cancelLabel: string;
    danger: boolean;
    resolve: (ok: boolean) => void;
  } | null>(null);
  const askConfirm = useCallback((
    message: string,
    opts?: { confirmLabel?: string; cancelLabel?: string; danger?: boolean }
  ): Promise<boolean> => {
    return new Promise((resolve) => setConfirmRequest({
      message,
      confirmLabel: opts?.confirmLabel ?? 'Discard',
      cancelLabel: opts?.cancelLabel ?? 'Cancel',
      danger: opts?.danger ?? true,
      resolve,
    }));
  }, []);
  const resolveConfirm = useCallback((ok: boolean) => {
    confirmRequest?.resolve(ok);
    setConfirmRequest(null);
  }, [confirmRequest]);

  // Closing a tab is the one place multi-tab editing can still genuinely
  // discard work (unlike opening, which always adds/focuses a tab instead
  // of overwriting one) -- guard it on `isDirty`.
  const closeTabWithGuard = useCallback(async (id: string) => {
    const tab = useIDEStore.getState().tabs.find((t) => t.id === id);
    if (!tab) return;
    if (tab.isDirty) {
      const discard = await askConfirm(
        `"${tab.name}" has unsaved changes. Discard them and close the tab?`,
        { confirmLabel: 'Discard', cancelLabel: 'Cancel', danger: true }
      );
      if (!discard) return;
    }
    storeCloseFile(id);
  }, [askConfirm, storeCloseFile]);

  const [workspacePath, setWorkspacePath] = useState<string | null>(null);
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [isLoadingWorkspace, setIsLoadingWorkspace] = useState(false);
  const [executionState, setExecutionState] = useState<ExecutionState>({
    isRunning: false,
    progress: 0,
    error: null,
  });
  const [terminalLines, setTerminalLines] = useState<string[]>([]);
  const [variables, setVariables] = useState<Variable[]>([]);
  const [numericVariables, setNumericVariables] = useState<PlotVar[]>([]);
  // `data:` URIs of figures produced by the most recent run (see
  // `ExecuteResponse.plots` / `collect_plot_outputs`). Replaced wholesale on
  // every run rather than accumulated, matching how the Terminal/Variables
  // panels already treat each Run as showing that run's own fresh state.
  const [figureImages, setFigureImages] = useState<string[]>([]);
  /** Keep figures from earlier runs instead of replacing them.
   *
   *  Off by default, because the common case is iterating on one figure and
   *  a pile of near-identical drafts is noise. On, it is how you compare a
   *  change against what you had -- which is otherwise impossible without
   *  saving files by hand. */
  const [keepFigures, setKeepFigures] = useState(false);
  /** Which run's results the panels are currently showing.
   *
   *  Two runs started in quick succession used to race: the panels showed
   *  whichever FINISHED last, so a slow earlier run could overwrite a fast
   *  later one and leave the figure disagreeing with the code on screen.
   *  A run whose token is stale by the time it returns is discarded. */
  const runToken = useRef(0);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [rightPanelOpen, setRightPanelOpen] = useState(true);
  const [cursorPosition, setCursorPosition] = useState({ line: 1, column: 1 });
  // The most recent run's error output, kept around even after a LATER run
  // succeeds -- unlike `executionState.error` (which a fresh successful run
  // resets to null), this is what the mascot's "why did that fail?" context
  // reads, so the answer stays available after the user has already moved
  // on to fixing/re-running the script.
  const [lastError, setLastError] = useState<string | null>(null);

  // ---- AI assist state (fix-error / generate / transform) ----
  // The editor's current selection, kept live via CodeEditor's
  // `onSelectionChange` (see that component's own doc comment) -- read at
  // the moment the "AI Assist" button is clicked, not re-read afterward,
  // since the user may click around the review modal (losing the Monaco
  // selection) before deciding whether to apply the suggestion.
  const [selection, setSelection] = useState<{
    text: string;
    range: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number };
  } | null>(null);
  // The instruction prompt bar for "AI Assist" (generate/transform, Task 3).
  const [aiPromptOpen, setAiPromptOpen] = useState(false);
  // Command Palette (Ctrl+Shift+P). `mascotOpenToken` is bumped by its
  // "Open Mascot" command to pop the mascot chat panel open externally
  // (see Mascot's own `openRequest` prop doc comment).
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  // AI Provider Settings modal (Local/OpenAI/Anthropic + API keys) -- see
  // `LlmProviderSettings.tsx`. Opened from the command palette's
  // "AI Provider Settings" entry.
  const [llmSettingsOpen, setLlmSettingsOpen] = useState(false);
  // Last backend that actually answered an AI Assist request, and whether
  // it was a fallback (see `llm_bridge.rs`'s `LlmAnswer`) -- surfaced as a
  // toast so a hosted-provider failure is visible, not silent, even though
  // the feature keeps working via the local fallback.
  const [lastLlmBackendNote, setLastLlmBackendNote] = useState<{ backend: string; fellBack: boolean; note?: string } | null>(null);
  // `undefined` (not e.g. 0) until the first real request -- Mascot's own
  // `openRequest` prop treats `undefined` as "no request yet" and any
  // defined value as one, so starting at a concrete number would pop the
  // panel open the instant the app mounts.
  const [mascotOpenToken, setMascotOpenToken] = useState<number | undefined>(undefined);
  const [aiInstruction, setAiInstruction] = useState('');
  // The shared review-before-apply modal (used by BOTH the fix-error button
  // and the generate/transform button) -- `null` when closed. `applyRange`
  // is `null` for a fix (replaces the whole buffer) or a fresh generate
  // (inserts at the cursor via `insertRequest`), and set to the captured
  // selection range for a transform (replaces just that range).
  const [aiReview, setAiReview] = useState<{
    title: string;
    subtitle?: string;
    original: string;
    suggested: string | null;
    loading: boolean;
    error: string | null;
    applyRange: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number } | null;
  } | null>(null);
  const [replaceRangeRequest, setReplaceRangeRequest] = useState<{
    text: string;
    range: { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number };
    token: number;
  } | null>(null);

  // Real, on-disk catalog examples and snippets (see `get_examples_dir` /
  // `list_directory` in src-tauri/src/main.rs). Both start empty and load
  // once on mount; outside a real Tauri window (e.g. `npm run dev` in a
  // plain browser tab) `invoke` rejects and both stay empty -- the
  // sections below render nothing rather than an error, same tolerance
  // `openFolderDialog`/`saveFileDialog` already have for that environment.
  const [catalogEntries, setCatalogEntries] = useState<CatalogEntry[]>([]);
  /** Which catalog category groups are expanded. Absent means "use the
   *  default": the first group open, the rest collapsed. */
  const [openCategories, setOpenCategories] = useState<Partial<Record<CatalogCategory, boolean>>>({});
  const [snippetEntries, setSnippetEntries] = useState<SnippetEntry[]>([]);
  const [snippetInsertRequest, setSnippetInsertRequest] = useState<{ text: string; token: number } | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const catalogDir = await invoke<string>('get_examples_dir');
        const entries = await invoke<FileInfo[]>('list_directory', { path: catalogDir });
        if (cancelled) return;

        const scripts = entries
          .filter(e => !e.is_dir && e.name.toLowerCase().endsWith('.qu'))
          .map(e => ({ name: e.name, path: e.path }))
          .sort((a, b) => a.name.localeCompare(b.name));
        setCatalogEntries(scripts);

        const snippetsDir = entries.find(e => e.is_dir && e.name === 'snippets');
        if (!snippetsDir) return;
        const snippetFiles = await invoke<FileInfo[]>('list_directory', { path: snippetsDir.path });
        if (cancelled) return;

        const snippets = await Promise.all(
          snippetFiles
            .filter(e => !e.is_dir && e.name.toLowerCase().endsWith('.qu'))
            .map(async (e): Promise<SnippetEntry> => {
              let name = e.name.replace(/\.qu$/i, '');
              let desc = '';
              try {
                const content = await invoke<string>('open_file', { path: e.path });
                for (const line of content.split(/\r\n|\r|\n/).slice(0, 5)) {
                  const nameMatch = line.match(SNIPPET_NAME_RE);
                  const descMatch = line.match(SNIPPET_DESC_RE);
                  if (nameMatch) name = nameMatch[1].trim();
                  if (descMatch) desc = descMatch[1].trim();
                }
              } catch {
                // Fall back to the filename-derived name with no description.
              }
              return { name, desc, path: e.path };
            })
        );
        if (!cancelled) {
          snippets.sort((a, b) => a.name.localeCompare(b.name));
          setSnippetEntries(snippets);
        }
      } catch {
        // No real Tauri backend (e.g. a plain browser preview) -- leave
        // both lists empty rather than surfacing an error in the terminal.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Clicking a Catalog entry opens it in its own tab (or focuses the
  // existing tab for that path), same as any other file open.
  const loadCatalogFile = async (entry: CatalogEntry) => {
    try {
      const content = await invoke<string>('open_file', { path: entry.path });
      openOrFocusTab({ id: entry.path, name: entry.name, path: entry.path, content });
      addTerminalLine(`Catalog: loaded ${entry.name}`);
    } catch (error: any) {
      addTerminalLine(`❌ Failed to load ${entry.name}: ${error.message ?? error}`, 'error');
    }
  };

  // Clicking a Snippet inserts its body at the current cursor position
  // instead (via CodeEditor's `insertRequest` prop) -- it's a fragment
  // meant to drop into code already being written, not a whole script.
  const insertSnippet = async (entry: SnippetEntry) => {
    try {
      const content = await invoke<string>('open_file', { path: entry.path });
      setSnippetInsertRequest({ text: stripSnippetHeader(content), token: Date.now() });
      addTerminalLine(`Snippet: inserted ${entry.name}`);
    } catch (error: any) {
      addTerminalLine(`❌ Failed to insert ${entry.name}: ${error.message ?? error}`, 'error');
    }
  };

  const terminalRef = useRef<HTMLDivElement>(null);

  // Auto-scroll terminal
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [terminalLines]);

  // Execute code
  const executeCode = useCallback(async (codeToRun: string = code) => {
    // Claim this run. Anything that returns holding an older token is a
    // run the user has already superseded.
    runToken.current += 1;
    const myToken = runToken.current;
    setExecutionState({ isRunning: true, progress: 0, error: null });
    addTerminalLine(`> Executing...`);
    
    try {
      const start = performance.now();
      
      const response = await invoke<ExecuteResponse>('execute_code', {
        request: {
          code: codeToRun,
          file_path: null,
        }
      });
      
      const elapsed = performance.now() - start;

      // Figures land in the response regardless of whether the script also
      // hit a runtime error partway through (e.g. `plot(x, y)` followed by
      // an unrelated later error still produced a figure) -- set this
      // unconditionally, matching `output`'s own "show whatever we got"
      // handling below rather than gating it on success.
      if (myToken !== runToken.current) {
        // A newer run started while this one was still going. Its results
        // are the ones that match the code on screen; drop these.
        return;
      }
      const plots = response.plots ?? [];
      setFigureImages((prev) => (keepFigures ? prev.concat(plots) : plots));
      setVariables(response.variables ?? []);
      setNumericVariables(response.data ?? []);

      // Show output
      if (response.output) {
        response.output.split('\n').forEach(line => {
          if (line.trim()) addTerminalLine(line);
        });
      }
      
      if (response.error) {
        setExecutionState({
          isRunning: false,
          progress: 100,
          error: response.error,
          elapsed_ms: elapsed,
        });
        setLastError(response.error);
        addTerminalLine(`❌ Error: ${response.error}`, 'error');
      } else {
        setExecutionState({ 
          isRunning: false, 
          progress: 100, 
          error: null,
          elapsed_ms: elapsed,
        });
        addTerminalLine(`✓ Completed in ${elapsed.toFixed(2)} ms`, 'success');
      }
      
    } catch (error: any) {
      setExecutionState({
        isRunning: false,
        progress: 100,
        error: error.message,
      });
      setLastError(error.message);
      addTerminalLine(`❌ Execution failed: ${error.message}`, 'error');
    }
  }, [code, keepFigures]);

  // Builds the "cheap context" the brief asks for: the current buffer plus
  // the last error, when there is one -- no RAG, no symbol indexing, just
  // the obviously relevant text so "why did this fail?" can actually work.
  // `llm_bridge.rs` on the Rust side additionally tail-truncates this (see
  // `CONTEXT_CHAR_BUDGET`), so a huge file is safe to pass through whole.
  const buildMascotContext = useCallback((): string | null => {
    const parts: string[] = [];
    if (code.trim()) parts.push(`Current file:\n${code}`);
    if (lastError) parts.push(`Last error:\n${lastError}`);
    return parts.length > 0 ? parts.join('\n\n') : null;
  }, [code, lastError]);

  // Every AI Assist command now returns `{ text, backend, fellBack, note }`
  // (see `llm_bridge.rs`'s `LlmAnswer`) instead of a bare string, so the
  // frontend can show which backend actually answered -- this helper
  // records that into `lastLlmBackendNote` for the fallback banner, and
  // returns just the text for callers that only need the reply itself.
  const noteLlmAnswer = useCallback((answer: { backend: string; fellBack: boolean; note?: string | null }) => {
    setLastLlmBackendNote({ backend: answer.backend, fellBack: answer.fellBack, note: answer.note ?? undefined });
  }, []);

  const llmChat = useCallback(
    async (question: string): Promise<string> => {
      const answer = await invoke<{ text: string; backend: string; fellBack: boolean; note?: string | null }>('llm_chat', {
        request: { prompt: question, context: buildMascotContext() },
      });
      noteLlmAnswer(answer);
      return answer.text;
    },
    [buildMascotContext, noteLlmAnswer]
  );

  // Inline-completion bridge for CodeEditor's `onInlineComplete` (see that
  // component's own doc comment) -- `max_tokens: 16` matches
  // `llm_bridge.rs`'s own `COMPLETE_DEFAULT_MAX_TOKENS`, spelt out here
  // rather than omitted so this call site states its own intent (a short,
  // one-line-ish completion, not a paragraph) instead of silently
  // depending on the Rust side's current default. Deliberately does NOT
  // call `noteLlmAnswer` -- ghost-text fires on every keystroke pause, so
  // surfacing a fallback banner for every single completion would be much
  // noisier than useful; the chat/fix/transform call sites (much lower
  // frequency, and each one is a deliberate user action) are where that
  // visibility actually matters.
  const llmComplete = useCallback(async (prefixCode: string): Promise<string> => {
    const answer = await invoke<{ text: string; backend: string; fellBack: boolean; note?: string | null }>('llm_complete', {
      request: { prefix: prefixCode, max_tokens: 16 },
    });
    return answer.text;
  }, []);

  // "Fix with AI" -- triggered from the inline error banner (rendered only
  // while `executionState.error` is set, see the JSX below). Deliberately
  // reads `executionState.error`, NOT `lastError`: `executeCode` clears
  // `executionState.error` back to `null` the instant a NEW run starts (see
  // its own first `setExecutionState` call), so this is guaranteed to be
  // the error from the run that's actually still showing on screen, never a
  // stale one left over from an earlier failed attempt the user has since
  // edited past. `code` is read live from React state at call time (it's
  // updated on every `onChange` keystroke, not debounced -- see
  // `CodeEditor`'s `onChange={(val) => onChange?.(val || '')}`), so this
  // always sends the buffer as it actually looks right now.
  const handleFixError = useCallback(() => {
    const currentError = executionState.error;
    if (!currentError) return;
    const originalCode = code;
    setAiReview({
      title: 'Fix with AI',
      subtitle: 'Suggested fix for the error below',
      original: originalCode,
      suggested: null,
      loading: true,
      error: null,
      applyRange: null,
    });
    invoke<{ text: string; backend: string; fellBack: boolean; note?: string | null }>('llm_fix_error', {
      request: { code: originalCode, error: currentError },
    })
      .then((answer) => {
        noteLlmAnswer(answer);
        setAiReview((prev) =>
          prev && prev.applyRange === null && prev.title === 'Fix with AI'
            ? { ...prev, suggested: answer.text, loading: false }
            : prev
        );
      })
      .catch((err: any) => {
        setAiReview((prev) =>
          prev && prev.title === 'Fix with AI'
            ? { ...prev, loading: false, error: err?.message ?? String(err) }
            : prev
        );
      });
  }, [code, executionState.error, noteLlmAnswer]);

  // "AI Assist" -- Task 3's generate/transform button. With a live
  // selection captured (see `onSelectionChange` below), this transforms
  // just that selection; with none, it generates brand-new code from the
  // instruction alone and inserts it at the cursor. Both modes go through
  // the same `llm_transform_code` command and the same review-before-apply
  // modal as the fix-error flow.
  const submitAiInstruction = useCallback(() => {
    const instruction = aiInstruction.trim();
    if (!instruction) return;
    const activeSelection = selection; // snapshot -- see this state's own doc comment
    const reviewTitle = activeSelection ? 'Transform selection' : 'Generate code';
    setAiPromptOpen(false);
    setAiInstruction('');
    setAiReview({
      title: reviewTitle,
      subtitle: instruction,
      original: activeSelection ? activeSelection.text : '',
      suggested: null,
      loading: true,
      error: null,
      applyRange: activeSelection ? activeSelection.range : null,
    });
    invoke<{ text: string; backend: string; fellBack: boolean; note?: string | null }>('llm_transform_code', {
      request: {
        instruction,
        code: code.trim() ? code : null,
        selection: activeSelection ? activeSelection.text : null,
      },
    })
      .then((answer) => {
        noteLlmAnswer(answer);
        setAiReview((prev) =>
          prev && prev.loading && prev.title === reviewTitle ? { ...prev, suggested: answer.text, loading: false } : prev
        );
      })
      .catch((err: any) => {
        setAiReview((prev) =>
          prev && prev.loading && prev.title === reviewTitle
            ? { ...prev, loading: false, error: err?.message ?? String(err) }
            : prev
        );
      });
  }, [aiInstruction, selection, code, noteLlmAnswer]);

  // Apply step, shared by both AI review flows. A fix or a from-scratch
  // generate has `applyRange === null`: a fix replaces the WHOLE buffer
  // (the model was asked to return a complete corrected script), a
  // from-scratch generate inserts at the current cursor via the existing
  // `insertRequest` mechanism (already used by the Snippets picker). A
  // transform has a captured `applyRange` and goes through the new
  // `replaceRangeRequest` mechanism instead, since it must replace exactly
  // the selection that was sent, not wherever the cursor happens to be now.
  const applyAiReview = useCallback(() => {
    if (!aiReview || aiReview.suggested === null || !activeFileId) return;
    if (aiReview.applyRange) {
      setReplaceRangeRequest({ text: aiReview.suggested, range: aiReview.applyRange, token: Date.now() });
    } else if (aiReview.title === 'Fix with AI') {
      updateFileContent(activeFileId, aiReview.suggested);
    } else {
      setSnippetInsertRequest({ text: aiReview.suggested, token: Date.now() });
    }
    addTerminalLine(`✓ AI suggestion applied (${aiReview.title})`, 'success');
    setAiReview(null);
  }, [aiReview, activeFileId, updateFileContent]);

  const discardAiReview = useCallback(() => {
    setAiReview(null);
  }, []);

  // Run a single `#%%` cell. `payload.prefixCode` is already cells
  // [0..cell.index] concatenated (see qu-ui-components' `utils/cells.ts`
  // `getPrefixSource`) -- `execute_code` spawns a fresh `qu.exe run
  // <tempfile>` process per call with no persistent state between calls,
  // so re-running every earlier cell in the same process is how a later
  // cell actually sees an earlier cell's variables. That means prior
  // cells' side effects (prints, plots) re-fire on every "Run Cell", which
  // is a deliberate, documented trade-off (see BACKLOG.md), not a bug.
  const handleRunCell = useCallback((payload: RunCellPayload) => {
    const label = payload.cell.title || `#${payload.cell.index + 1}`;
    addTerminalLine(`> Running cell ${label} (re-executing cells 1-${payload.cell.index + 1})`);
    executeCode(payload.prefixCode);
  }, [executeCode]);

  const addTerminalLine = (line: string, type: 'info' | 'error' | 'success' = 'info') => {
    const prefix = type === 'error' ? '❌ ' : type === 'success' ? '✓ ' : '';
    setTerminalLines(prev => [...prev, `${prefix}${line}`]);
  };

  const clearTerminal = () => setTerminalLines([]);

  // Write the whole figure collection somewhere durable. Until this
  // existed the collection lived only in memory -- each run's directory is
  // deleted as soon as its figures are read -- so "Keep" kept them only
  // until the app closed.
  const saveFigureCollection = useCallback(
    async (format: 'svg' | 'png') => {
      if (!figureImages.length) return;
      try {
        // A directory, not a file: the collection is many figures, and
        // naming each one in turn would be the worst part of using this.
        const { open } = await import('@tauri-apps/api/dialog');
        const dir = await open({
          directory: true,
          multiple: false,
          title: `Choose a folder for ${figureImages.length} ${format.toUpperCase()} figure(s)`,
        });
        if (typeof dir !== 'string') return;
        const files = await buildFigureFiles(figureImages, format);
        const written = await invoke<string[]>('save_figures', { dir, files });
        addTerminalLine(`saved ${written.length} figure(s) to ${dir}`);
      } catch (err) {
        addTerminalLine(`could not save figures: ${err}`);
      }
    },
    [figureImages, addTerminalLine],
  );
  const currentFilePath = currentTab?.path ?? null;

  // Opens (or focuses) a tab for one of the hardcoded template lists
  // (Signal Processing / DSP / ML) -- keyed by a synthetic `template:...`
  // id since these have no on-disk path.
  const loadTemplate = (id: string, name: string, template: string) => {
    openOrFocusTab({ id: `template:${id}`, name: `${name}.qu`, path: null, content: template });
    addTerminalLine(`Template loaded: ${name}`);
  };

  // If the active tab has a real path (from a workspace folder, a catalog
  // open, or a prior save-as), save it in place via the native `save_file`
  // command. Otherwise there's no path to write to yet, so prompt a native
  // "Save As" dialog first -- this is the ONE save path; both cases end up
  // calling `save_file`, never a browser download (which a WebView has no
  // real place to put anyway).
  // `forceDialog` is what separates Save As (Ctrl+Shift+S) from Save: the
  // only difference between them is whether the tab's existing path is
  // allowed to short-circuit the dialog. Everything after that -- the
  // write, the rename, the terminal line, the disk re-baseline -- is
  // deliberately ONE path, so the two can never drift apart.
  const handleSave = useCallback(async (forceDialog = false) => {
    if (!currentTab) return;
    let path = forceDialog ? null : currentTab.path;
    if (!path) {
      try {
        path = await saveFileDialog();
      } catch (error: any) {
        addTerminalLine(`❌ Save dialog failed: ${error.message ?? error}`, 'error');
        return;
      }
      if (!path) return; // user cancelled
    }

    try {
      await invoke<void>('save_file', { path, content: currentTab.content });
      const name = path.split(/[\\/]/).pop() || currentTab.name;
      markFileSaved(currentTab.id, { path, name });
      addTerminalLine(`✓ Saved: ${path}`, 'success');
    } catch (error: any) {
      addTerminalLine(`❌ Save failed: ${error.message ?? error}`, 'error');
    }
  }, [currentTab, markFileSaved]);

  // Ctrl+Shift+S. A thin wrapper rather than its own implementation, for
  // the reason above -- and it is a `useCallback` of its own so the
  // window-level key handler's dependency list stays honest.
  const handleSaveAs = useCallback(() => handleSave(true), [handleSave]);

  // F1: answer "what is this?" about the symbol under the cursor, in the
  // terminal, without leaving the editor.
  //
  // `help()` is the engine's own introspection, and the function to reach
  // for -- `available("name")`, which CLAUDE.md still recommends, is the
  // SERIAL PORT builtin and errors on a string.
  //
  // Invokes `execute_code` directly rather than going through this app's
  // own `executeCode`, deliberately: `executeCode` publishes its results
  // into the workspace, so routing help through it would clear the
  // variables and figures panels of whatever the user last ran, just to
  // print a docstring. Nothing here touches `executionState`, `variables`
  // or `figureImages`.
  const handleHelpRequest = useCallback(async (symbol: string | null) => {
    if (!symbol) {
      addTerminalLine('F1: put the cursor on a name to look it up, e.g. inside `fft` or `linspace`.');
      return;
    }
    addTerminalLine(`help("${symbol}")`);
    try {
      const response = await invoke<ExecuteResponse>('execute_code', {
        request: { code: `help("${symbol}")`, file_path: null },
      });
      const text = (response.output || '').trimEnd();
      if (text) {
        text.split('\n').forEach((line) => addTerminalLine(line));
      } else {
        // A name the engine does not know is a normal answer to the
        // question asked, not a failure of the lookup -- say so plainly
        // rather than surfacing an interpreter error the user did not
        // cause.
        addTerminalLine(`no entry for "${symbol}" -- check the spelling, or the builtin index in website/`);
      }
    } catch (error: any) {
      addTerminalLine(`help failed: ${error.message ?? error}`, 'error');
    }
  }, []);

  // Ctrl+E. Binds to the figure export that already existed rather than
  // adding a second one; SVG because it is lossless and the format
  // `FIGURE_FORMATS` leads with. PDF and TikZ are deliberately not offered
  // from a keystroke: they cannot be produced from what the panel holds --
  // they are rendered by the engine, not converted -- so a key that
  // silently gave you a converted approximation would be worse than no key.
  const handleExportFigures = useCallback(async () => {
    if (!figureImages.length) {
      addTerminalLine('nothing to export yet -- run a script with plot or savefig first');
      return;
    }
    await saveFigureCollection('svg');
  }, [figureImages, saveFigureCollection]);

  // Ctrl+P. Prints the FIGURES, not the source.
  //
  // "the code OR the figure", per Ahmed. Which one is decided by focus,
  // because that is the only signal available that means anything: the
  // editor having focus is the user saying they are working in the code.
  // With focus anywhere else, figures win when there are any -- they are
  // what Qu produces and what usually wants printing -- and the code is
  // the fallback when there is nothing plotted yet, so the key always
  // does SOMETHING rather than reporting an empty state.
  //
  // Whichever it picks is named in the terminal. A print key that quietly
  // prints the wrong thing is worse than one that asks, and saying what it
  // did is the cheap version of asking.
  //
  // Uses the page's own print path with a print-only stylesheet (see
  // `#qu-print-area` below) rather than opening a window or an iframe:
  // a WebView is not a browser, `window.open` is not dependable in one,
  // and a popup blocked silently would look like a dead key.
  const [printMode, setPrintMode] = useState<'code' | 'figures'>('figures');
  // Bumped to ask for a print. The actual `window.print()` happens in an
  // effect keyed on this, because `printMode` must be RENDERED before the
  // print dialog snapshots the page -- calling print() in the same tick
  // prints the previous mode's content.
  const [printRequest, setPrintRequest] = useState(0);

  const handlePrint = useCallback(() => {
    const editorFocused = !!(document.activeElement as HTMLElement | null)?.closest?.('.monaco-editor');
    const mode: 'code' | 'figures' = editorFocused || figureImages.length === 0 ? 'code' : 'figures';
    if (mode === 'code' && !code.trim()) {
      addTerminalLine('nothing to print -- the buffer is empty and no figures have been drawn');
      return;
    }
    setPrintMode(mode);
    setPrintRequest((n) => n + 1);
    addTerminalLine(
      mode === 'code'
        ? `printing ${currentTab?.name ?? 'the current buffer'}`
        : `printing ${figureImages.length} figure(s)`,
    );
  }, [figureImages, code, currentTab]);

  useEffect(() => {
    if (printRequest === 0) return;
    window.print();
  }, [printRequest]);

  // ---- "changed on disk" detection ----
  // Deliberately a poll, not a watcher: see `utils/diskWatch.ts` for why,
  // and for the two-stage rule (a stat is a gate, the bytes are the
  // verdict) that keeps a `git checkout` restoring identical content from
  // interrupting anyone.
  //
  // Everything here is keyed by PATH rather than tab id, because the tab
  // id is promoted to the path on first save (see `markFileSaved`) and a
  // baseline filed under the pre-save id would be orphaned by that.
  const diskBaselinesRef = useRef<Map<string, DiskBaseline>>(new Map());
  const [diskAlert, setDiskAlert] = useState<
    { path: string; kind: 'modified' | 'deleted'; diskContent: string; baselineContent: string } | null
  >(null);
  const [diskDiff, setDiskDiff] = useState<{ path: string; buffer: string; disk: string } | null>(null);

  // Record what is on disk RIGHT NOW as the new "unchanged". Used to seed a
  // path the first time it is seen, after a save this app itself performed
  // (which of course moves mtime), and when the user answers the banner
  // either way -- both "reload" and "keep mine" mean "stop telling me about
  // this particular disk state", they just differ in which buffer wins.
  const rebaselineFromDisk = useCallback(async (path: string): Promise<string | null> => {
    try {
      const stat = await invoke<DiskStat>('file_stat', { path });
      const content = stat.exists ? await invoke<string>('open_file', { path }) : '';
      diskBaselinesRef.current.set(path, { stat, content });
      return stat.exists ? content : null;
    } catch {
      // A stat that cannot be taken is not evidence of a change; leave the
      // baseline alone and try again on the next tick.
      return null;
    }
  }, []);

  const checkDiskForPath = useCallback(async (path: string, bufferContent: string) => {
    let stat: DiskStat;
    try {
      stat = await invoke<DiskStat>('file_stat', { path });
    } catch {
      return;
    }
    const baselines = diskBaselinesRef.current;
    const baseline = baselines.get(path);
    const verdict = classifyStat(baseline, stat);

    if (verdict === 'unchanged') {
      // First sight of this path: seed it. This bootstrap is why no open
      // path (file tree, dialog, catalog, examples) has to be hooked --
      // whichever way a file got here, its first poll establishes the
      // baseline and says nothing.
      if (!baseline) await rebaselineFromDisk(path);
      return;
    }

    if (verdict === 'deleted') {
      baselines.set(path, { stat, content: baseline?.content ?? '' });
      setDiskAlert({ path, kind: 'deleted', diskContent: '', baselineContent: baseline?.content ?? '' });
      return;
    }

    // 'recheck' / 'recreated' -- something moved, but only the bytes can
    // say whether it matters.
    let diskContent: string;
    try {
      diskContent = await invoke<string>('open_file', { path });
    } catch {
      return;
    }

    // Disk and buffer agree, so there is nothing to tell the user no
    // matter what moved the mtime. This is the case that covers THIS
    // app's own Ctrl+S -- without it every save would raise a "changed on
    // disk" banner one tick later, since the recorded baseline still held
    // the pre-save bytes. It equally covers an external tool that wrote
    // exactly what the user already has.
    if (diskContent === bufferContent) {
      baselines.set(path, { stat, content: diskContent });
      setDiskAlert((current) => (current && current.path === path ? null : current));
      return;
    }

    if (confirmChange(baseline, diskContent) === 'none') {
      // Rewritten with identical content. Adopt the new stat silently so
      // this does not re-read the file on every subsequent tick, and
      // retract any banner that was standing for an earlier state.
      baselines.set(path, { stat, content: diskContent });
      setDiskAlert((current) => (current && current.path === path ? null : current));
      return;
    }

    // Adopt the new STAT but keep the old CONTENT as the baseline: that
    // stops the next tick re-reading the file, while leaving the recorded
    // "what we last agreed with disk about" intact for the diff and for
    // `hasLocalEdits`.
    baselines.set(path, { stat, content: baseline?.content ?? '' });
    setDiskAlert({
      path,
      kind: 'modified',
      diskContent,
      // The BYTES we last agreed with disk about, kept so the banner can
      // re-evaluate `hasLocalEdits` against the LIVE buffer on every
      // render rather than freezing the answer at detection time.
      baselineContent: baseline?.content ?? '',
    });
  }, [rebaselineFromDisk]);

  // Keep the buffer text in a ref so the poll can read it without making
  // the interval re-subscribe on every keystroke.
  const bufferForDiskCheckRef = useRef(code);
  bufferForDiskCheckRef.current = code;

  useEffect(() => {
    const path = currentTab?.path;
    // Outside a Tauri window there is no IPC to ask, and `invoke` would
    // log a failure per tick. An untitled buffer has no disk state yet.
    if (!path || typeof (window as any).__TAURI_IPC__ === 'undefined') return;

    let cancelled = false;
    const check = () => {
      if (!cancelled) void checkDiskForPath(path, bufferForDiskCheckRef.current);
    };
    // The background interval is the only part gated on focus. A Studio
    // sitting behind another window has no user to interrupt, and the
    // moment that actually matters -- coming back to it -- is covered by
    // the `focus` listener, which fires a check of its own.
    const tick = () => {
      if (document.hasFocus()) check();
    };

    // Deliberately NOT focus-gated: switching to a file is an explicit
    // request to look at it, so it gets a fresh answer regardless of what
    // the OS thinks about window focus. Gating this too made the first
    // check silently do nothing whenever `document.hasFocus()` was false
    // for reasons unrelated to the user's attention.
    check();
    window.addEventListener('focus', check);
    const timer = window.setInterval(tick, DISK_POLL_MS);
    return () => {
      cancelled = true;
      window.removeEventListener('focus', check);
      window.clearInterval(timer);
    };
  }, [currentTab?.path, checkDiskForPath]);

  // Take the on-disk version, discarding whatever is in the buffer. The
  // tab comes back clean because it now matches the file exactly.
  const reloadFromDisk = useCallback(async () => {
    const tab = currentTab;
    if (!tab?.path) return;
    const disk = await rebaselineFromDisk(tab.path);
    if (disk === null) {
      addTerminalLine(`❌ Could not re-read ${tab.path}`, 'error');
      return;
    }
    updateFileContent(tab.id, disk);
    markFileSaved(tab.id);
    setDiskAlert(null);
    setDiskDiff(null);
    addTerminalLine(`✓ Reloaded from disk: ${tab.path}`, 'success');
  }, [currentTab, rebaselineFromDisk, updateFileContent, markFileSaved]);

  // Keep the buffer. The file on disk is left exactly as it is -- this
  // does NOT write anything -- but we stop warning about this particular
  // disk state, so a LATER external change still raises the banner again.
  const keepMyVersion = useCallback(async () => {
    const path = diskAlert?.path;
    setDiskAlert(null);
    setDiskDiff(null);
    if (path) await rebaselineFromDisk(path);
  }, [diskAlert, rebaselineFromDisk]);

  // The WebView's own native Ctrl+S ("Save Page As") fires at the browser
  // level regardless of which element has focus, and Monaco's
  // `editor.addCommand` (see CodeEditor.tsx) only intercepts the key while
  // the editor itself is focused -- e.g. with focus in the sidebar/terminal,
  // or if the editor's own handling doesn't fully suppress the browser
  // default, Ctrl+S falls through to the WebView's save-page UI instead of
  // this app's save. A capturing, window-level listener guarantees we see
  // the key first and can always preventDefault, funneling into the exact
  // same `handleSave` used everywhere else (no second, divergent save path).
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const isSaveChord = (event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey
        && event.key.toLowerCase() === 's';
      if (!isSaveChord) return;
      event.preventDefault();
      event.stopPropagation();
      handleSave();
    };
    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [handleSave]);

  // Ctrl+O / the "Open" toolbar button. A browser `<input type=file>` gives
  // no reliable absolute path (even inside a Tauri WebView), so this always
  // opens as a path-less tab, keyed by filename -- reopening the
  // same-named file focuses that tab rather than duplicating it, the same
  // "focus an existing tab" behavior as any other open path.
  const handleOpen = async () => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.qu,.txt';
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
        const text = await file.text();
        openOrFocusTab({ id: `local:${file.name}`, name: file.name, path: null, content: text });
        addTerminalLine(`✓ Loaded: ${file.name}`);
      }
    };
    input.click();
  };

  // Ctrl+N / "New File": start a blank, untitled tab. `handleSave` already
  // has a dedicated "no path yet" branch (see `saveFileDialog` above) that
  // prompts a native Save As the first time this buffer is saved, exactly
  // like any other untitled-file flow. Opening a new tab never discards an
  // existing one, so this needs no unsaved-changes guard.
  const handleNewFile = () => {
    newUntitledFile();
    addTerminalLine('New file');
  };

  // Open a real folder via the native picker, then populate the file tree
  // from it using the `list_directory` Tauri command.
  const handleOpenFolder = async () => {
    let selected: string | null;
    try {
      selected = await openFolderDialog();
    } catch (error: any) {
      addTerminalLine(`❌ Folder picker failed: ${error.message ?? error}`, 'error');
      return;
    }
    if (!selected) return; // user cancelled

    setIsLoadingWorkspace(true);
    addTerminalLine(`> Opening folder: ${selected}`);
    try {
      const tree = await buildFileTree(selected);
      setWorkspacePath(selected);
      setFileTree(tree);
      addTerminalLine(`✓ Workspace loaded: ${selected}`, 'success');
    } catch (error: any) {
      addTerminalLine(`❌ Failed to read folder: ${error.message ?? error}`, 'error');
    } finally {
      setIsLoadingWorkspace(false);
    }
  };

  // Clicking a file in the tree loads its contents into the editor via the
  // `open_file` Tauri command. Folders just expand/collapse (handled inside
  // FileTree itself).
  const handleFileTreeSelect = async (node: FileNode) => {
    if (node.type !== 'file') return;
    try {
      const content = await invoke<string>('open_file', { path: node.path });
      openOrFocusTab({ id: node.path, name: node.name, path: node.path, content });
      addTerminalLine(`✓ Opened: ${node.path}`, 'success');
    } catch (error: any) {
      addTerminalLine(`❌ Failed to open ${node.path}: ${error.message ?? error}`, 'error');
    }
  };

  // Ctrl+O (open), Ctrl+N (new tab), Ctrl+W (close active tab, through the
  // same unsaved-changes guard as clicking a tab's own close button), and
  // Ctrl+Tab (cycle to the next tab) -- all currently dead/missing despite
  // the toolbar's own "Open (Ctrl+O)" tooltip promising otherwise. A single
  // capturing, window-level listener (same pattern as the Ctrl+S handler
  // above) so these work regardless of which element has focus.
  //
  // The 2026-09-16 keymap additions this listener reaches for are held in a
  // ref rather than listed as dependencies. Both halves of that matter:
  //
  //  - `executeCode` closes over `code`, and `handleSaveAs` (via
  //    `handleSave`) over `currentTab`. BOTH change on every keystroke, so
  //    as dependencies they would tear down and re-add a window listener on
  //    every character typed.
  //  - But merely omitting them -- which is what this effect already did
  //    for `handleOpen`/`handleNewFile` -- is a stale closure: F5 would run
  //    whatever the buffer held when the listener was installed. Silent and
  //    wrong beats noisy and right only if you never notice.
  //
  // A ref reassigned each render gives the stable subscription and the
  // current closure at once, the same pattern this file already uses for
  // `onRunCellRef` and `bufferForDiskCheckRef`.
  const keymapActionsRef = useRef({
    executeCode, handleNewFile, handleSaveAs, reloadFromDisk,
    handleExportFigures, handlePrint,
  });
  keymapActionsRef.current = {
    executeCode, handleNewFile, handleSaveAs, reloadFromDisk,
    handleExportFigures, handlePrint,
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // ---- Function keys, handled BEFORE the Ctrl guard below ----
      // F5/F2/F1 carry no modifier, so the `ctrlKey` test would return
      // early on them.
      //
      // F5 and Ctrl+F5 are the WebView's own reload and hard-reload. Until
      // now they reloaded Qu Studio itself -- losing every unsaved buffer
      // behind nothing but a `beforeunload` prompt -- so claiming them is
      // a safety fix as much as a keybinding.
      if (event.key === 'F5') {
        event.preventDefault();
        if (event.ctrlKey || event.metaKey) {
          // Run-cell needs the cursor, which only the editor knows, so it
          // is bound in CodeEditor.tsx. Deliberately NO stopPropagation:
          // this exists purely to kill the hard-reload, and the event must
          // still reach Monaco's own Ctrl+F5 command.
          return;
        }
        keymapActionsRef.current.executeCode();
        return;
      }
      if (event.key === 'F2' && !event.ctrlKey && !event.metaKey && !event.altKey) {
        // Monaco binds F2 to `editor.action.rename`, which in a Qu buffer
        // has no rename provider behind it and can only report failure.
        // Ahmed's ruling drops it, so this DOES stopPropagation -- unlike
        // the F5 guard above -- to keep the editor from also acting.
        event.preventDefault();
        event.stopPropagation();
        setCommandPaletteOpen((o) => !o);
        return;
      }

      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === 'r' && !event.shiftKey) {
        // The WebView's reload, claimed for the same reason as F5: it
        // discarded the whole session. Reads as "reload the file from
        // disk", the editor sense, which is the action this app already
        // has -- see `reloadFromDisk`, shared with the changed-on-disk
        // banner so there is one reload path, not two.
        event.preventDefault();
        event.stopPropagation();
        void keymapActionsRef.current.reloadFromDisk();
      } else if (key === 't' && !event.shiftKey) {
        // A second binding for new-tab alongside Ctrl+N, per the keymap.
        event.preventDefault();
        keymapActionsRef.current.handleNewFile();
      } else if (key === 'e' && !event.shiftKey) {
        event.preventDefault();
        void keymapActionsRef.current.handleExportFigures();
      } else if (key === 'p' && !event.shiftKey) {
        // Ctrl+P is the WebView's own print, which would print the entire
        // app chrome -- sidebar, terminal and all. Claimed so it prints the
        // figures instead. Ctrl+SHIFT+P is the command palette and is
        // handled further down; the `!event.shiftKey` guard is what keeps
        // these two apart.
        event.preventDefault();
        event.stopPropagation();
        keymapActionsRef.current.handlePrint();
      } else if (key === 's' && event.shiftKey) {
        event.preventDefault();
        event.stopPropagation();
        void keymapActionsRef.current.handleSaveAs();
      } else if (key === 'o' && !event.shiftKey) {
        event.preventDefault();
        handleOpen();
      } else if (key === 'n' && !event.shiftKey) {
        event.preventDefault();
        handleNewFile();
      } else if (key === 'w' && !event.shiftKey) {
        event.preventDefault();
        const id = useIDEStore.getState().activeFileId;
        if (id) closeTabWithGuard(id);
      } else if (event.key === 'Tab') {
        event.preventDefault();
        cycleActiveFile(event.shiftKey ? -1 : 1);
      } else if (key === 'p' && event.shiftKey) {
        // Ctrl+Shift+P -- the conventional Command Palette binding, not
        // claimed by anything else in this app.
        event.preventDefault();
        setCommandPaletteOpen((o) => !o);
      }
    };
    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [closeTabWithGuard, cycleActiveFile]);

  // Best-effort unsaved-work guard on app/window close. A native browser API
  // rather than Tauri's window-close event: it works identically during
  // `npm run dev` in a plain browser tab and inside the packaged desktop
  // WebView, with no extra allowlist permissions needed.
  useEffect(() => {
    const onBeforeUnload = (event: BeforeUnloadEvent) => {
      if (tabs.some((t) => t.isDirty)) {
        event.preventDefault();
        event.returnValue = '';
      }
    };
    window.addEventListener('beforeunload', onBeforeUnload);
    return () => window.removeEventListener('beforeunload', onBeforeUnload);
  }, [tabs]);

  // ---- Autosave + crash recovery (localStorage) ----
  // Deliberately simple: one debounced snapshot of the ACTIVE tab only (not
  // a full multi-tab history), and a one-time restore offer on startup.
  // This is a safety net for an unclean exit (crash/force-quit), not a
  // substitute for `handleSave` -- a clean save/close never leaves a
  // snapshot behind to restore.
  const AUTOSAVE_KEY = 'qu-studio.autosave.v1';
  const restoreCheckedRef = useRef(false);
  useEffect(() => {
    if (restoreCheckedRef.current) return;
    restoreCheckedRef.current = true;
    try {
      const raw = localStorage.getItem(AUTOSAVE_KEY);
      if (!raw) return;
      const snapshot = JSON.parse(raw) as { name: string; path: string | null; content: string };
      if (!snapshot?.content?.trim()) return;
      askConfirm(
        `Recover unsaved work from "${snapshot.name}" found from a previous session?`,
        { confirmLabel: 'Restore', cancelLabel: 'Discard', danger: false }
      ).then((restore) => {
        localStorage.removeItem(AUTOSAVE_KEY);
        if (!restore) return;
        const restoredId = snapshot.path ?? `recovered:${Date.now()}`;
        openOrFocusTab({ id: restoredId, name: snapshot.name, path: snapshot.path, content: snapshot.content });
        // The recovered content is exactly what's already in the tab, but
        // it hasn't been saved to disk (that's the whole point of a crash
        // snapshot) -- flip `isDirty` on so the tab/close guard treat it as
        // unsaved work rather than a clean, already-saved file.
        updateFileContent(restoredId, snapshot.content);
        addTerminalLine('✓ Restored unsaved work from a previous session', 'success');
      });
    } catch {
      // Corrupt/foreign snapshot -- ignore rather than block startup on it.
      localStorage.removeItem(AUTOSAVE_KEY);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounced snapshot of the active tab, only while it actually has
  // unsaved changes -- a clean tab needs no crash-recovery copy since the
  // file on disk already has its content.
  useEffect(() => {
    if (!currentTab || !currentTab.isDirty) return;
    const timer = setTimeout(() => {
      try {
        localStorage.setItem(AUTOSAVE_KEY, JSON.stringify({
          name: currentTab.name,
          path: currentTab.path,
          content: currentTab.content,
        }));
      } catch {
        // Storage full/unavailable -- autosave is best-effort, never fatal.
      }
    }, 1500);
    return () => clearTimeout(timer);
  }, [currentTab]);

  // Clears the crash-recovery snapshot once there is nothing dirty left to
  // protect (e.g. right after a save) -- otherwise a stale snapshot from an
  // already-saved tab would trigger a pointless restore prompt next launch.
  useEffect(() => {
    if (!tabs.some((t) => t.isDirty)) {
      try { localStorage.removeItem(AUTOSAVE_KEY); } catch { /* best-effort */ }
    }
  }, [tabs]);

  // ---- Command Palette (Ctrl+Shift+P) ----
  // Adapts real, already-wired app actions into `CommandPalette`'s
  // `Command` shape rather than inventing a second command-registration
  // mechanism.
  const paletteCommands: PaletteCommand[] = useMemo(() => [
    { id: 'open', label: 'Open File', description: 'Open a file into a new tab', shortcut: ['Ctrl', 'O'], icon: <FolderOpen size={16} />, category: 'File', action: handleOpen },
    { id: 'new-tab', label: 'New Tab', description: 'Start a blank, untitled tab', shortcut: ['Ctrl', 'N'], icon: <FilePlus size={16} />, category: 'File', action: handleNewFile },
    { id: 'save', label: 'Save', description: 'Save the active tab', shortcut: ['Ctrl', 'S'], icon: <Save size={16} />, category: 'File', action: () => handleSave() },
    {
      id: 'close-tab', label: 'Close Tab', description: 'Close the active tab', shortcut: ['Ctrl', 'W'], icon: <X size={16} />, category: 'File',
      action: () => { if (activeFileId) closeTabWithGuard(activeFileId); },
    },
    { id: 'next-tab', label: 'Next Tab', description: 'Cycle to the next open tab', shortcut: ['Ctrl', 'Tab'], icon: <ChevronRight size={16} />, category: 'File', action: () => cycleActiveFile(1) },
    { id: 'run', label: 'Run', description: 'Execute the active tab', shortcut: ['Ctrl', 'Enter'], icon: <Play size={16} />, category: 'Run', action: () => executeCode() },
    {
      id: 'run-cell', label: 'Run First Cell',
      description: 'Run cell 1 of the active tab (place the cursor in another cell and press Shift+Enter to run that one instead)',
      icon: <Play size={16} />, category: 'Run',
      action: () => {
        const cells = parseCells(code);
        if (cells.length === 0) return;
        handleRunCell({ cell: cells[0], prefixCode: getPrefixSource(code, cells, 0) });
      },
    },
    { id: 'toggle-theme', label: 'Toggle Theme', description: `Switch to ${resolvedTheme === 'dark' ? 'light' : 'dark'}`, icon: <Sparkles size={16} />, category: 'View', action: toggleTheme },
    { id: 'open-mascot', label: 'Open Mascot', description: 'Ask the Qu assistant about your code', icon: <Sparkles size={16} />, category: 'AI', action: () => setMascotOpenToken(Date.now()) },
    {
      id: 'ai-assist', label: selection ? 'Transform Selection with AI' : 'Generate Code with AI',
      icon: <Wand2 size={16} />, category: 'AI', action: () => setAiPromptOpen(true),
    },
    {
      id: 'ai-provider-settings', label: 'AI Provider Settings',
      description: 'Pick Local/OpenAI/Anthropic for AI Assist and save an API key',
      icon: <Settings size={16} />, category: 'AI', action: () => setLlmSettingsOpen(true),
    },
  ], [handleSave, activeFileId, closeTabWithGuard, cycleActiveFile, code, resolvedTheme, toggleTheme, selection]);

  return (
    <div
      className="qu-shell h-screen w-screen overflow-hidden flex flex-col"
      style={{ background: 'var(--qu-shell-bg)', color: 'var(--qu-text)' }}
    >
      {/* Top Bar */}
      <div
        className="h-14 flex items-center justify-between px-4 border-b backdrop-blur-xl"
        style={{ background: 'var(--qu-shell-panel)', borderColor: 'var(--qu-border)' }}
      >
        <div className="flex items-center gap-3">
          {/* Wordmark. The tagline sat at the same weight as the mode tabs
              beside it and competed with them for the first read; it is
              the one thing on this bar nobody ever needs to act on, so it
              drops to the muted tone and gets out of the way. */}
          <div className="flex items-center gap-2.5">
            <div
              className="w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0"
              style={{ background: 'var(--qu-accent-solid)' }}
            >
              <Cpu size={18} className="text-white" />
            </div>
            <div className="leading-tight">
              <div className="font-semibold text-[15px] tracking-tight">Qu Studio</div>
              <div className="text-[11px]" style={{ color: 'var(--qu-muted)' }}>
                Signal Processing &amp; ML IDE
              </div>
            </div>
          </div>

          <div className="h-7 w-px" style={{ background: 'var(--qu-border)' }} />

          {/* Mode Tabs.
              Was seven verbatim copies of the same button, each carrying
              its own colour ternary -- which is how the inactive hover
              came to be `bg-[#1c2740]/50`, a dark navy that only ever
              made sense against the old dark editor and that painted a
              near-black smear across the white top bar in light mode.
              One list, one `.qu-modetab` rule (index.css), so an eighth
              tab cannot disagree with the other seven. `aria-selected` on
              a real tablist also gives the row the semantics it was
              already miming with colour alone. */}
          <div className="flex items-center gap-0.5" role="tablist" aria-label="Workspace mode">
            {MODE_TABS.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                role="tab"
                aria-selected={activeTab === id}
                onClick={() => setActiveTab(id)}
                className="qu-modetab"
              >
                <Icon size={14} />
                {label}
              </button>
            ))}
          </div>
        </div>

        {/* Right-hand action cluster.
            Eight controls used to sit here at one uniform weight, which
            made the bar read as a wall of icons with a blue button
            somewhere in it. They are three different KINDS of action, so
            they are now three groups separated by the same hairline the
            left side already uses: what you are doing now (AI, Run),
            what you do to the file (new/save/history/open), and what is
            always available (palette, reference, theme). Grouping is the
            cheapest hierarchy available on a toolbar and it costs no
            space. */}
        <div className="flex items-center gap-1.5">
          {/* AI Assist: generate new code from a description (no selection)
              or transform the current selection (Task 3). Opens the small
              instruction bar below the top bar rather than a full modal --
              the actual review happens in AiDiffModal once a suggestion
              comes back. */}
          <button
            onClick={() => setAiPromptOpen((o) => !o)}
            title={selection ? 'Transform selected code with AI' : 'Generate code with AI'}
            aria-pressed={aiPromptOpen}
            className="qu-bar-button"
          >
            <Wand2 size={16} />
            {selection ? 'Transform' : 'AI Assist'}
          </button>

          {/* Run Button — hidden for the Designer tab, which owns its own
              Design/Code/Run workflow (`GuiDesignerPanel`'s own Run
              sub-tab); this button executes the CODE EDITOR's content,
              which has nothing to do with a designed form and was
              confusing to have alongside a second, unrelated "Run". */}
          {activeTab !== 'designer' && (
          <button
            onClick={() => executeCode()}
            disabled={executionState.isRunning}
            className="qu-bar-button is-primary"
          >
            {executionState.isRunning ? (
              <Loader size={16} className="animate-spin" />
            ) : (
              <Play size={16} />
            )}
            {executionState.isRunning ? 'Running…' : 'Run'}
          </button>
          )}

          <div className="h-6 w-px mx-1" style={{ background: 'var(--qu-border)' }} />

          <button onClick={handleNewFile} className="qu-bar-icon" title="New File (Ctrl+N)">
            <FilePlus size={17} />
          </button>

          <button onClick={() => handleSave()} className="qu-bar-icon" title="Save (Ctrl+S)">
            <Save size={17} />
          </button>

          {currentTab?.path && (
            <button
              onClick={() => setShowVersionHistory(true)}
              className="qu-bar-icon"
              title="Version history"
            >
              <History size={17} />
            </button>
          )}

          <button onClick={handleOpen} className="qu-bar-icon" title="Open (Ctrl+O)">
            <FolderOpen size={17} />
          </button>

          <div className="h-6 w-px mx-1" style={{ background: 'var(--qu-border)' }} />

          <button
            onClick={() => setCommandPaletteOpen(true)}
            className="qu-bar-icon"
            title="Command Palette (Ctrl+Shift+P)"
          >
            <CommandIcon size={17} />
          </button>

          <button
            onClick={() => setShowHelpBrowser(true)}
            className="qu-bar-icon"
            title="Browse Qu's builtin reference"
          >
            <BookOpen size={17} />
          </button>

          <ThemeToggle className="qu-bar-icon" />
        </div>
      </div>

      {/* AI Assist instruction bar -- Task 3 (generate/transform). Shows
          whether there's a live selection (transform mode) or not (generate
          mode) so the user knows which behavior Enter/Submit will trigger. */}
      {aiPromptOpen && (
        <div
          className="flex items-center gap-2.5 px-4 py-2 border-b flex-shrink-0"
          style={{ background: 'var(--qu-shell-panel)', borderColor: 'var(--qu-border)' }}
        >
          <Sparkles size={14} style={{ color: 'var(--qu-accent)' }} />
          <span className="text-xs whitespace-nowrap" style={{ color: 'var(--qu-muted)' }}>
            {selection ? 'Transform selection:' : 'Generate code:'}
          </span>
          <input
            autoFocus
            value={aiInstruction}
            onChange={(e) => setAiInstruction(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submitAiInstruction();
              if (e.key === 'Escape') setAiPromptOpen(false);
            }}
            placeholder={selection ? 'e.g. vectorize this loop' : 'e.g. plot a damped sine wave'}
            className="qu-field flex-1 text-sm"
          />
          <button
            onClick={submitAiInstruction}
            disabled={!aiInstruction.trim()}
            className="qu-bar-button is-primary"
          >
            Go
          </button>
          <button
            onClick={() => setAiPromptOpen(false)}
            className="qu-bar-icon"
            aria-label="Close AI assist bar"
          >
            <X size={15} />
          </button>
        </div>
      )}

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {activeTab === 'interactive' ? (
          <InteractiveModePanel theme={resolvedTheme as 'light' | 'dark'} invoke={invoke} />
        ) : activeTab === 'seriplot' ? (
          <LiveSerialPlotPanel theme={resolvedTheme as 'light' | 'dark'} invoke={invoke} />
        ) : activeTab === 'designer' ? (
          <GuiDesignerPanel theme={resolvedTheme as 'light' | 'dark'} invoke={invoke} />
        ) : activeTab === 'dsp' ? (
          <DspWorkbenchPanel theme={resolvedTheme as 'light' | 'dark'} invoke={invoke} />
        ) : (
        <>
        {/* Sidebar - Templates */}
        {sidebarOpen && (
          <div
            className="w-72 flex-shrink-0 border-r overflow-y-auto"
            style={{ background: 'var(--qu-shell-panel)', borderColor: 'var(--qu-border)' }}
          >
            {activeTab === 'code' && (
              <div className="p-3">
                <div className="qu-side-heading">
                  Signal Processing
                </div>
                {SIGNAL_PROCESSING_EXAMPLES.map(tool => (
                  <button
                    key={tool.id}
                    onClick={() => loadTemplate(tool.id, tool.name, tool.template)}
                    className="qu-side-card mb-2"
                  >
                    <div className="flex items-center gap-2 mb-1">
                      {tool.icon}
                      <span className="font-medium">{tool.name}</span>
                    </div>
                    <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
                      {tool.description}
                    </div>
                  </button>
                ))}

                {catalogEntries.length > 0 && (
                  <>
                    <div className="qu-side-heading mt-5">
                      Catalog
                    </div>
                    {/* Grouped by subject and collapsible, rather than 57
                        raw filenames in one alphabetical run -- Arduino's
                        `File > Examples` is the reference. Each group starts
                        collapsed except the first, so the list opens as a
                        readable table of contents instead of a wall. */}
                    {groupCatalog(catalogEntries).map(({ category, items }, groupIndex) => {
                      const open = openCategories[category] ?? groupIndex === 0;
                      const Icon = CATEGORY_ICONS[CATEGORY_ICON[category]] ?? FileCode;
                      return (
                        <div key={category} className="mb-1">
                          <button
                            onClick={() => setOpenCategories(prev => ({ ...prev, [category]: !open }))}
                            className="qu-side-row w-full text-left px-2 py-1.5 text-xs font-medium flex items-center gap-2"
                          >
                            <ChevronRight
                              size={12}
                              className={cn("transition-transform duration-150 flex-shrink-0", open && "rotate-90")}
                            />
                            <Icon size={13} className="flex-shrink-0 opacity-70" />
                            <span className="truncate">{category}</span>
                            <span className="ml-auto tabular-nums text-[10px]" style={{ color: 'var(--qu-muted)' }}>
                              {items.length}
                            </span>
                          </button>
                          {open && items.map(entry => (
                            <button
                              key={entry.path}
                              onClick={() => loadCatalogFile(entry)}
                              title={entry.name}
                              className="qu-side-row w-full text-left pl-8 pr-3 py-1.5 text-sm flex items-center gap-2"
                            >
                              <span className="truncate">{catalogTitle(entry.name)}</span>
                            </button>
                          ))}
                        </div>
                      );
                    })}
                  </>
                )}

                {snippetEntries.length > 0 && (
                  <>
                    <div className="qu-side-heading mt-5">
                      Snippets (insert at cursor)
                    </div>
                    {snippetEntries.map(entry => (
                      <button
                        key={entry.path}
                        onClick={() => insertSnippet(entry)}
                        title={entry.desc || entry.name}
                        className="qu-side-card mb-1 px-3 py-2"
                      >
                        <div className="text-sm font-medium">{entry.name}</div>
                        {entry.desc && (
                          <div className="text-xs" style={{ color: 'var(--qu-muted)' }}>
                            {entry.desc}
                          </div>
                        )}
                      </button>
                    ))}
                  </>
                )}
              </div>
            )}

            {activeTab === 'ml' && (
              <div className="p-3">
                <div className="qu-side-heading">
                  Machine Learning
                </div>
                {ML_ALGORITHMS.map(algo => (
                  <button
                    key={algo.id}
                    onClick={() => loadTemplate(algo.id, algo.name, algo.template)}
                    className="qu-side-card mb-2"
                  >
                    <div className="flex items-center gap-2 mb-1">
                      <Network size={14} />
                      <span className="font-medium">{algo.name}</span>
                    </div>
                    <div className="qu-chip" data-kind={algo.category}>
                      {algo.category}
                    </div>
                  </button>
                ))}
              </div>
            )}

            {activeTab === 'files' && (
              <div className="p-3 h-full flex flex-col">
                <button
                  onClick={handleOpenFolder}
                  disabled={isLoadingWorkspace}
                  className="qu-side-action w-full mb-3"
                >
                  {isLoadingWorkspace ? (
                    <Loader size={14} className="animate-spin" />
                  ) : (
                    <FolderOpen size={14} />
                  )}
                  {isLoadingWorkspace ? 'Loading...' : 'Open Folder'}
                </button>

                {workspacePath ? (
                  <>
                    <div className="text-xs mb-2 truncate" style={{ color: 'var(--qu-muted)' }} title={workspacePath}>
                      {workspacePath}
                    </div>
                    <div className="flex-1 overflow-hidden">
                      <FileTree
                        files={fileTree}
                        selectedFile={currentFilePath ?? undefined}
                        onSelect={handleFileTreeSelect}
                        theme={resolvedTheme as 'light' | 'dark'}
                      />
                    </div>
                  </>
                ) : (
                  <div className="qu-inspector qu-empty" data-theme={resolvedTheme}>
                    <Folder size={28} />
                    <strong>Browse a project</strong>
                    <p>
                      Open a folder and its .qu files appear here, one click from
                      the editor.
                    </p>
                  </div>
                )}
              </div>
            )}
          </div>
        )}

        {/* Editor Area */}
        <div className="flex-1 flex flex-col min-w-0">
          <TabBar
            tabs={tabs.map((t) => ({ id: t.id, name: t.name, isDirty: t.isDirty }))}
            activeTabId={activeFileId ?? ''}
            onTabClick={(id) => setActiveFileId(id)}
            onTabClose={(id) => closeTabWithGuard(id)}
          />
          <CodeEditor
            value={code}
            onChange={(val) => activeFileId && updateFileContent(activeFileId, val)}
            language="qu"
            onRun={() => executeCode()}
            onRunCell={handleRunCell}
            onHelpRequest={handleHelpRequest}
            onSave={() => handleSave()}
            insertRequest={snippetInsertRequest}
            replaceRangeRequest={replaceRangeRequest}
            onSelectionChange={setSelection}
            theme={resolvedTheme as any}
            onInlineComplete={llmComplete}
            // Automatic (type-and-wait) triggering is OFF: measured
            // latency for even a short completion on this CPU-only 1.1B
            // model is multiple seconds (see IMPL.md's dated entry for the
            // actual number), and ghost text arriving that long after the
            // user stopped typing pops in somewhere they've already moved
            // on from -- worse than no suggestion. Explicit trigger only,
            // via Ctrl+Alt+Space (see CodeEditor's own command binding).
            autoTriggerInlineComplete={false}
          />

          {/* "Changed on disk" banner. Deliberately an inline strip in the
              same shape as the error banner below -- NOT a modal. An
              external change is not an emergency and the user may well
              want to keep reading their own buffer before deciding, so
              nothing here steals focus, blocks typing, or has to be
              dismissed before the editor can be used again. It is also
              per-path (`diskAlert.path === currentTab?.path`), so
              switching tabs hides it rather than showing one file's
              warning above another file's text.

              Tailwind's amber-300/red-400 text on an amber-500/10 fill is
              legible on a dark editor and very nearly invisible on a white
              one -- the tint and the type were both picked in the one
              theme this banner ever got looked at in. The `.qu-banner`
              token pair carries a readable foreground in both. */}
          {diskAlert && diskAlert.path === currentTab?.path && (
            <div className="qu-banner" data-kind="warning">
              <AlertCircle size={16} className="flex-shrink-0" />
              <span className="text-xs truncate flex-1" title={diskAlert.path}>
                {diskAlert.kind === 'deleted'
                  ? 'This file was deleted on disk. Your buffer is still here — save to write it back.'
                  : hasLocalEdits(diskAlert.baselineContent, code)
                    ? 'Changed on disk, and you have unsaved edits here. Compare before choosing.'
                    : 'Changed on disk since it was opened here.'}
              </span>
              {diskAlert.kind === 'modified' ? (
                <>
                  <button
                    onClick={() => setDiskDiff({ path: diskAlert.path, buffer: code, disk: diskAlert.diskContent })}
                    className="qu-banner-secondary flex-shrink-0"
                  >
                    Show diff
                  </button>
                  <button
                    onClick={reloadFromDisk}
                    className="qu-banner-action flex-shrink-0"
                  >
                    <RotateCcw size={12} />
                    Reload
                  </button>
                </>
              ) : (
                // A deleted file has no version to compare against and
                // nothing to reload, so "Show diff"/"Reload" would both be
                // dead. The one useful action is putting the buffer back on
                // disk, which is just an ordinary save.
                <button
                  onClick={() => handleSave()}
                  className="qu-banner-action flex-shrink-0"
                >
                  <Save size={12} />
                  Save it back
                </button>
              )}
              <button
                onClick={keepMyVersion}
                className="qu-banner-secondary flex-shrink-0"
                title={diskAlert.kind === 'deleted'
                  ? 'Stop warning about this deletion; the buffer stays open either way'
                  : 'Leave the file on disk untouched and stop warning about this change'}
              >
                {/* "Keep mine" only makes sense against a rival version.
                    With the file gone there is nothing to keep it instead
                    OF, so the same button is just a dismissal there. */}
                {diskAlert.kind === 'deleted' ? 'Dismiss' : 'Keep mine'}
              </button>
            </div>
          )}

          {/* Error banner + "Fix with AI" -- Task 2. Only rendered while
              `executionState.error` is set (the CURRENT run's error; it's
              cleared to `null` the instant a new run starts, and replaced
              with the new run's own error/`null` once that resolves -- see
              `executeCode`), so this disappears the moment the error it's
              about is no longer the live state, rather than lingering with
              stale text. */}
          {executionState.error && (
            <div className="qu-banner" data-kind="error">
              <AlertCircle size={16} className="flex-shrink-0" />
              <span className="text-xs truncate flex-1 font-mono" title={executionState.error}>
                {executionState.error}
              </span>
              <button
                onClick={handleFixError}
                className="qu-banner-action flex-shrink-0"
              >
                <Sparkles size={12} />
                Fix with AI
              </button>
            </div>
          )}

          {/* Terminal.
              Its dark background was #0e1524 -- a navy the top bar,
              sidebar and modals around it never used -- so the bottom
              third of the window belonged to a different application than
              the top. It is now the shell's sunken surface, the same
              relationship the light theme already had (#f5f5f5 under
              #f9f9f7). Its header also matches the sidebar's section
              headings rather than inventing a fourth label style. */}
          <div
            className="h-48 border-t overflow-hidden flex flex-col"
            style={{ background: 'var(--qu-shell-sunken)', borderColor: 'var(--qu-border)' }}
          >
            <div
              className="flex items-center justify-between px-3 py-2 border-b"
              style={{ borderColor: 'var(--qu-border)', color: 'var(--qu-muted)' }}
            >
              <div className="flex items-center gap-2">
                <TerminalIcon size={13} />
                <span className="text-[11px] font-semibold uppercase tracking-wider">Terminal</span>
              </div>
              <button onClick={clearTerminal} className="qu-bar-icon" style={{ width: 24, height: 24 }} title="Clear terminal">
                <X size={14} />
              </button>
            </div>
            <div
              ref={terminalRef}
              className="flex-1 overflow-y-auto px-3 py-2 font-mono text-[12.5px] leading-relaxed"
              style={{ color: 'var(--qu-text)' }}
            >
              {terminalLines.map((line, i) => (
                <div key={i} className="py-0.5 whitespace-pre-wrap break-words">{line}</div>
              ))}
              {terminalLines.length === 0 && (
                <div style={{ color: 'var(--qu-muted)' }}>
                  Ready &mdash; press Run, or Ctrl+Enter.
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Workspace inspection uses the same full numeric output as Interactive Mode. */}
        {rightPanelOpen && (
          <aside className="qu-inspector qu-workspace-inspector" data-theme={resolvedTheme} aria-label="Workspace inspector">
            <FigureViewer
              images={figureImages}
              theme={resolvedTheme as 'light' | 'dark'}
              target={figureTarget}
              onTargetChange={setFigureTargetInCode}
              onInsertCode={insertFigureCode}
              keepFigures={keepFigures}
              onKeepFiguresChange={setKeepFigures}
              onSaveCollection={saveFigureCollection}
            />
            <VariableExplorer variables={variables} numericData={numericVariables} theme={resolvedTheme as 'light' | 'dark'} isRunning={executionState.isRunning} />
          </aside>
        )}
        </>
        )}
      </div>

      {/* Status Bar. Same navy/shell mismatch as the terminal above.
          Tabular numerals stop the cursor position and the elapsed time
          from shifting the fields beside them on every keystroke and on
          every run -- the usual reason a status bar jitters. */}
      <div
        className="h-6 flex items-center justify-between px-3 text-[11px] border-t flex-shrink-0"
        style={{
          background: 'var(--qu-shell-sunken)',
          borderColor: 'var(--qu-border)',
          color: 'var(--qu-muted)',
        }}
      >
        <div className="flex items-center gap-3.5 tabular-nums">
          <span>Ln {cursorPosition.line}, Col {cursorPosition.column}</span>
          <span>Qu</span>
          <span>UTF-8</span>
        </div>
        <div className="flex items-center gap-3.5 tabular-nums">
          {executionState.isRunning && (
            <span className="flex items-center gap-1.5" style={{ color: 'var(--qu-accent)' }}>
              <Loader size={10} className="animate-spin" />
              Running
            </span>
          )}
          {/* `elapsed_ms && ...` rather than a real predicate printed a
              bare `0` into the bar for any run that finished inside a
              millisecond -- JSX renders the number 0, it does not read it
              as "absent". Hidden while a run is in flight, too: the old
              row showed "Running..." beside the PREVIOUS run's timing,
              which looks like a live clock and is not one. */}
          {executionState.elapsed_ms != null && !executionState.isRunning && (
            <span>{executionState.elapsed_ms.toFixed(2)} ms</span>
          )}
          <span>Qu Studio v0.1.0</span>
        </div>
      </div>

      <Mascot onAsk={llmChat} theme={resolvedTheme as 'light' | 'dark'} openRequest={mascotOpenToken} />

      <LlmProviderSettings
        open={llmSettingsOpen}
        onClose={() => setLlmSettingsOpen(false)}
        theme={resolvedTheme as 'light' | 'dark'}
        invoke={invoke}
      />

      {/* Visible fallback indicator (brief: "make the fallback visible to
          the user, not silent") -- a small dismissible toast-like banner
          naming whichever backend actually answered the last AI Assist
          request, shown a beat longer when it was a fallback (the user
          needs time to notice "OpenAI failed" is actionable, not just
          "answered by Local"). Auto-clears itself so it never becomes a
          permanent fixture the user has to consciously dismiss. */}
      {lastLlmBackendNote && (
        <LlmBackendToast note={lastLlmBackendNote} onDone={() => setLastLlmBackendNote(null)} theme={resolvedTheme as 'light' | 'dark'} />
      )}

      <CommandPalette
        commands={paletteCommands}
        isOpen={commandPaletteOpen}
        onClose={() => setCommandPaletteOpen(false)}
        theme={resolvedTheme as 'light' | 'dark'}
      />

      {/* Unsaved-changes guard -- shared by tab-close and the startup
          crash-recovery prompt (see `askConfirm`). */}
      {confirmRequest && (
        <div className="qu-scrim fixed inset-0 z-[60] flex items-center justify-center">
          <div
            className="w-[420px] rounded-xl border p-5"
            style={{
              background: 'var(--qu-shell-panel)',
              borderColor: 'var(--qu-border)',
              color: 'var(--qu-text)',
              boxShadow: '0 24px 64px rgb(0 0 0 / 35%)',
            }}
          >
            <div className="flex items-start gap-3 mb-4">
              <AlertCircle size={20} className="flex-shrink-0 mt-0.5" style={{ color: 'var(--qu-warning)' }} />
              <p className="text-sm leading-relaxed">{confirmRequest.message}</p>
            </div>
            <div className="flex justify-end gap-2">
              <button onClick={() => resolveConfirm(false)} className="qu-bar-button">
                {confirmRequest.cancelLabel}
              </button>
              <button
                onClick={() => resolveConfirm(true)}
                className={cn('qu-bar-button', confirmRequest.danger ? 'is-danger' : 'is-primary')}
              >
                {confirmRequest.confirmLabel}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Shared review-before-apply panel for both the "Fix with AI" (Task 2)
          and "AI Assist" generate/transform (Task 3) buttons -- nothing an
          AI call returns is ever written into the buffer without the user
          explicitly clicking Apply here. */}

      {/* Disk-vs-buffer comparison. Reuses the same review panel the AI
          suggestions go through -- its props are generic (title/original/
          suggested/apply/discard) despite the component's name, so this
          gets the identical LCS line diff and the identical "nothing is
          written into your buffer until you click Apply" guarantee,
          rather than a second, differently-behaved diff view. `original`
          is the buffer and `suggested` is the file, so Apply means "take
          the version on disk" -- exactly what Reload does. */}
      {diskDiff && (
        <AiDiffModal
          title="File changed on disk"
          subtitle={diskDiff.path}
          original={diskDiff.buffer}
          suggested={diskDiff.disk}
          onApply={reloadFromDisk}
          onDiscard={() => setDiskDiff(null)}
          theme={resolvedTheme as 'light' | 'dark'}
        />
      )}
      {aiReview && (
        <AiDiffModal
          title={aiReview.title}
          subtitle={aiReview.subtitle}
          original={aiReview.original}
          suggested={aiReview.suggested}
          loading={aiReview.loading}
          error={aiReview.error}
          onApply={applyAiReview}
          onDiscard={discardAiReview}
          theme={resolvedTheme as 'light' | 'dark'}
        />
      )}

      {showVersionHistory && currentTab?.path && (
        <VersionHistoryPanel
          path={currentTab.path}
          currentContent={currentTab.content}
          theme={resolvedTheme as 'light' | 'dark'}
          invoke={invoke}
          onRestore={(content) => currentTab && updateFileContent(currentTab.id, content)}
          onClose={() => setShowVersionHistory(false)}
        />
      )}

      {showHelpBrowser && (
        <HelpBrowser
          theme={resolvedTheme as 'light' | 'dark'}
          invoke={invoke}
          onClose={() => setShowHelpBrowser(false)}
        />
      )}

      {/* Ctrl+P print target. Hidden on screen, and the ONLY thing visible
          on paper -- `visibility` rather than `display` so the rest of the
          layout is not reflowed into the printed page, which is what makes
          a naive print-stylesheet spill the sidebar and terminal across
          the first sheet. `figureImages` are data URIs, so nothing is
          fetched while the print dialog is open. */}
      <style>{`
        @media screen { #qu-print-area { display: none; } }
        @media print {
          body * { visibility: hidden !important; }
          #qu-print-area, #qu-print-area * { visibility: visible !important; }
          #qu-print-area {
            position: absolute; left: 0; top: 0; width: 100%;
            background: #fff;
          }
          #qu-print-area img {
            display: block; max-width: 100%; height: auto;
            margin: 0 auto 24px; page-break-inside: avoid;
          }
          #qu-print-area h1 {
            font: 600 13pt/1.3 system-ui, sans-serif; color: #000;
            margin: 0 0 10pt; padding-bottom: 4pt;
            border-bottom: 1px solid #999;
          }
          #qu-print-area pre {
            font: 9pt/1.45 "Cascadia Mono", Consolas, monospace; color: #000;
            /* pre-wrap, not pre: a long line otherwise runs off the edge
               of the paper and is simply gone, which is the usual reason
               printed code is useless. */
            white-space: pre-wrap; word-break: break-word;
            margin: 0;
          }
        }
      `}</style>
      <div id="qu-print-area">
        {printMode === 'figures' && figureImages.map((src, i) => (
          <img key={i} src={src} alt={`Figure ${i + 1}`} />
        ))}
        {printMode === 'code' && (
          <>
            <h1>{currentTab?.name ?? 'untitled.qu'}</h1>
            {/* `white-space: pre-wrap` in the print CSS, so a long line
                wraps rather than being cut off at the paper's edge --
                which is the usual reason printed code is useless. */}
            <pre>{code}</pre>
          </>
        )}
      </div>
    </div>
  );
}

export default App;
