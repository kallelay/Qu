/**
 * Presentation metadata for the `catalog/*.qu` demo scripts.
 *
 * The sidebar used to list raw filenames, alphabetically, 57 deep --
 * `qu_ar_model_forecast.qu` sitting between `demo_signal.qu` and
 * `qu_bridge_shm.qu` with nothing to tell a signal-processing demo from a
 * concurrency one. Arduino's `File > Examples` is the reference Ahmed
 * named: grouped by subject, each entry readable at a glance.
 *
 * Kept as data here rather than as tags inside each `.qu` file so the
 * scripts stay plain Qu (they're meant to be readable, runnable examples,
 * not carriers of IDE metadata), and so a script with no entry still shows
 * up -- `categorize` falls back to a title derived from the filename and
 * an "Everything else" group, which means adding a demo never requires
 * touching this file to make it visible.
 */

export type CatalogCategory =
  | 'Getting started'
  | 'Signal processing'
  | 'Measurement & instrumentation'
  | 'Machine learning'
  | 'Statistics & estimation'
  | 'Computer vision'
  | 'Math & linear algebra'
  | 'Simulation'
  | 'Concurrency & performance'
  | 'Data & I/O'
  | 'Language & UI'
  | 'Everything else';

/** Order the groups appear in. Signal processing leads because it's the
 *  language's centre of gravity; "Everything else" is always last. */
export const CATEGORY_ORDER: CatalogCategory[] = [
  'Getting started',
  'Signal processing',
  'Measurement & instrumentation',
  'Machine learning',
  'Statistics & estimation',
  'Computer vision',
  'Math & linear algebra',
  'Simulation',
  'Concurrency & performance',
  'Data & I/O',
  'Language & UI',
  'Everything else',
];

/** Lucide icon name per category -- resolved in the component so this file
 *  stays free of JSX and can be unit-tested as plain data. */
export const CATEGORY_ICON: Record<CatalogCategory, string> = {
  'Getting started': 'PlayCircle',
  'Signal processing': 'AudioWaveform',
  'Measurement & instrumentation': 'Radio',
  'Machine learning': 'Network',
  'Statistics & estimation': 'BarChart3',
  'Computer vision': 'Layers',
  'Math & linear algebra': 'Binary',
  Simulation: 'Activity',
  'Concurrency & performance': 'Cpu',
  'Data & I/O': 'Database',
  'Language & UI': 'Sparkles',
  'Everything else': 'FileCode',
};

interface Meta {
  title: string;
  category: CatalogCategory;
}

/** Keyed by filename without the `.qu`. */
const META: Record<string, Meta> = {
  demo_hello: { title: 'Hello, Qu', category: 'Getting started' },
  demo_sine: { title: 'Plot a sine wave', category: 'Getting started' },
  demo_vectors: { title: 'Vectors & matrices', category: 'Getting started' },
  demo_signal: { title: 'Signal basics', category: 'Signal processing' },
  qu_fft_spectrum: { title: 'FFT spectrum', category: 'Signal processing' },
  qu_filter_design: { title: 'Filter design', category: 'Signal processing' },
  qu_signal_generators: { title: 'Waveform generators', category: 'Signal processing' },
  qu_peak_finding: { title: 'Peak finding', category: 'Signal processing' },
  qu_rc_filter: { title: 'RC filter response', category: 'Signal processing' },
  qu_multisine: { title: 'Multisine excitation', category: 'Signal processing' },
  qu_multisine_acceleration: { title: 'Multisine acceleration', category: 'Signal processing' },
  qu_interpolate_apply_cut: { title: 'Interpolate, apply & cut', category: 'Signal processing' },
  qu_bilinear_interp2: { title: 'Bilinear interpolation', category: 'Signal processing' },

  qu_compressed_sensing: { title: 'Compressed sensing', category: 'Signal processing' },
  qu_starry_night: { title: 'Starry Night, animated', category: 'Language & UI' },
  qu_sea_waves: { title: 'A sea, from the wave equation', category: 'Simulation' },
  qu_signal_explorer: { title: 'Signal explorer (sliders)', category: 'Signal processing' },
  qu_layers_schematic: { title: 'Layers: a rig schematic', category: 'Language & UI' },
  qu_noise_and_removal: { title: 'Noise, in and out', category: 'Signal processing' },
  qu_impedance_rlkk: { title: 'Impedance (rLKK)', category: 'Measurement & instrumentation' },
  qu_bridge_shm: { title: 'Bridge health monitoring', category: 'Measurement & instrumentation' },
  qu_serial_ports_discovery: { title: 'Serial port discovery', category: 'Measurement & instrumentation' },
  qu_serial_realtime: { title: 'Real-time serial capture', category: 'Measurement & instrumentation' },

  qu_ml_report: { title: 'ML report', category: 'Machine learning' },
  qu_classification_report: { title: 'Classification report', category: 'Machine learning' },
  qu_naive_bayes_classifier: { title: 'Naive Bayes classifier', category: 'Machine learning' },
  qu_sequential_xor: { title: 'Neural net: XOR', category: 'Machine learning' },
  qu_lstm_forecast: { title: 'LSTM forecast', category: 'Machine learning' },
  qu_gridworld_rl: { title: 'Reinforcement learning', category: 'Machine learning' },
  qu_pca_projection: { title: 'PCA projection', category: 'Machine learning' },
  qu_curve_fit: { title: 'Curve fitting', category: 'Machine learning' },
  qu_ar_model_forecast: { title: 'AR model forecast', category: 'Machine learning' },

  qu_kalman_tracking: { title: 'Kalman tracking', category: 'Statistics & estimation' },
  qu_particle_filter: { title: 'Particle filter', category: 'Statistics & estimation' },
  qu_hmm_healthy_fever: { title: 'Hidden Markov model', category: 'Statistics & estimation' },
  qu_markov_weather: { title: 'Markov chain', category: 'Statistics & estimation' },
  qu_random_walk: { title: 'Random walk', category: 'Statistics & estimation' },
  qu_corrcoef_heatmap: { title: 'Correlation heatmap', category: 'Statistics & estimation' },

  qu_image_blobs: { title: 'Blob detection', category: 'Computer vision' },
  qu_voronoi_diagram: { title: 'Voronoi diagram', category: 'Computer vision' },

  qu_linear_algebra: { title: 'Linear algebra', category: 'Math & linear algebra' },
  qu_qr_svd: { title: 'QR & SVD', category: 'Math & linear algebra' },
  qu_temperature_units: { title: 'Units & conversions', category: 'Math & linear algebra' },

  qu_fluid_simulation: { title: 'Fluid simulation', category: 'Simulation' },

  qu_gpu_matmul: { title: 'GPU matrix multiply', category: 'Concurrency & performance' },
  qu_distributed_pool: { title: 'Distributed worker pool', category: 'Concurrency & performance' },
  qu_mutex_semaphore: { title: 'Mutex & semaphore', category: 'Concurrency & performance' },
  qu_memoize_lazy: { title: 'Memoize & lazy eval', category: 'Concurrency & performance' },
  qu_sandbox_profiling: { title: 'Sandbox & profiling', category: 'Concurrency & performance' },

  qu_format_conversion: { title: 'JSON / CSV / XML', category: 'Data & I/O' },
  qu_file_extensions: { title: 'File formats', category: 'Data & I/O' },
  qu_http_get: { title: 'HTTP requests', category: 'Data & I/O' },
  qu_streamfile_streamurl: { title: 'Streaming files & URLs', category: 'Data & I/O' },
  qu_tcp_echo: { title: 'TCP echo server', category: 'Data & I/O' },
  qu_fifo_double_buffer: { title: 'FIFO & double buffer', category: 'Data & I/O' },
  qu_linked_list_and_graph: { title: 'Linked list & graph', category: 'Data & I/O' },

  qu_gui_controls: { title: 'GUI controls', category: 'Language & UI' },
  qu_interactive_sliders: { title: 'Interactive sliders', category: 'Language & UI' },
  qu_multiple_dispatch: { title: 'Multiple dispatch', category: 'Language & UI' },
  qu_operator_overload: { title: 'Operator overloading', category: 'Language & UI' },
  qu_watch_reactive: { title: 'Reactive watch', category: 'Language & UI' },
};

/** `qu_fft_spectrum.qu` -> `FFT spectrum`. Used verbatim for known entries
 *  and derived for unknown ones, so a newly added demo reads sensibly in
 *  the sidebar without anyone editing this file first. */
export function catalogTitle(fileName: string): string {
  const key = fileName.replace(/\.qu$/i, '');
  const known = META[key];
  if (known) return known.title;
  const words = key.replace(/^qu_/, '').replace(/_/g, ' ').trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function catalogCategory(fileName: string): CatalogCategory {
  return META[fileName.replace(/\.qu$/i, '')]?.category ?? 'Everything else';
}

/** Groups entries for the sidebar, in `CATEGORY_ORDER`, dropping empty
 *  groups so the list never shows a header with nothing under it. */
export function groupCatalog<T extends { name: string }>(
  entries: T[]
): Array<{ category: CatalogCategory; items: T[] }> {
  const buckets = new Map<CatalogCategory, T[]>();
  for (const entry of entries) {
    const cat = catalogCategory(entry.name);
    const list = buckets.get(cat) ?? [];
    list.push(entry);
    buckets.set(cat, list);
  }
  return CATEGORY_ORDER.filter((c) => buckets.has(c)).map((category) => ({
    category,
    items: (buckets.get(category) ?? []).sort((a, b) =>
      catalogTitle(a.name).localeCompare(catalogTitle(b.name))
    ),
  }));
}
