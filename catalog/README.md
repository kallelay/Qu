# The Qu catalog

Seventy-four runnable programs. Every one is a real script, not a snippet —
run any of them:

```
qu run catalog/demo_hello.qu
```

**All 74 were run to verify this index** (2026-09-11), and two problems that
turned up were fixed rather than documented around: `qu_fluid_simulation.qu`
only ran from the repository root, and nothing here was findable without an
index. One demo is merely slow, and is flagged in place below.

---

## Start here

In order. Each is short and each one runs.

| | | |
|---|---|---|
| 1 | [`demo_hello.qu`](demo_hello.qu) | Hello world, arithmetic, and string interpolation. **20 lines.** |
| 2 | [`demo_vectors.qu`](demo_vectors.qu) | Vectors and matrices, and why `*` and `.*` differ. |
| 3 | [`demo_sine.qu`](demo_sine.qu) | Your first figure: a time axis, a waveform, a plot. |
| 4 | [`demo_signal.qu`](demo_signal.qu) | Units, ranges, broadcasting and functions in one small script. |
| 5 | [`demo_timer.qu`](demo_timer.qu) | Timing code: several independent stopwatches at once. |
| 6 | [`demo_filter.qu`](demo_filter.qu) | A real job — lowpass a noisy measurement, then check it worked. |

After those six, pick whatever below matches what you actually want to do.

---

## Signals and filtering

| | |
|---|---|
| [`qu_fft_spectrum.qu`](qu_fft_spectrum.qu) | Complex numbers and the DFT. |
| [`qu_filter_design.qu`](qu_filter_design.qu) | IIR filter design, diagnostics, and application. |
| [`qu_rc_filter.qu`](qu_rc_filter.qu) | RC lowpass: step response and Bode plot. |
| [`qu_peak_finding.qu`](qu_peak_finding.qu) | `findpeaks()` on a noisy multi-tone signal. |
| [`qu_noise_and_removal.qu`](qu_noise_and_removal.qu) | Putting noise in, and taking it out again. |
| [`qu_signal_generators.qu`](qu_signal_generators.qu) | The `signals.` namespace: five standard waveforms. |
| [`qu_spectrogram_interactive.qu`](qu_spectrogram_interactive.qu) | Time-frequency analysis. |
| [`qu_interpolate_apply_cut.qu`](qu_interpolate_apply_cut.qu) | Signal-aware interpolation and elementwise application. |
| [`qu_signal_explorer.qu`](qu_signal_explorer.qu) | A filter you can feel, as a page you can send. |
| [`qu_fifo_double_buffer.qu`](qu_fifo_double_buffer.qu) | An acquisition loop with double buffering. |

### Multisine and crest factor

| | |
|---|---|
| [`qu_multisine.qu`](qu_multisine.qu) | Multisine synthesis, with a per-tone figure breakdown. |
| [`qu_multisine_acceleration.qu`](qu_multisine_acceleration.qu) | The same workload, three ways, timed. |

## Measurement and instruments

| | |
|---|---|
| [`qu_impedance_rlkk.qu`](qu_impedance_rlkk.qu) | Regularized linear Kramers-Kronig validation. |
| [`qu_bridge_shm.qu`](qu_bridge_shm.qu) | Bridge structural health monitoring. |
| [`qu_temperature_units.qu`](qu_temperature_units.qu) | Tracked physical units, including the awkward ones (degC). |
| [`qu_serial_ports_discovery.qu`](qu_serial_ports_discovery.qu) | Finding serial ports. |
| [`qu_serial_realtime.qu`](qu_serial_realtime.qu) | Real-time serial capture and processing. |

## Maths and linear algebra

| | |
|---|---|
| [`qu_linear_algebra.qu`](qu_linear_algebra.qu) | A tour of the matrix surface. |
| [`qu_qr_svd.qu`](qu_qr_svd.qu) | QR, SVD, and a least-squares solve. |
| [`qu_curve_fit.qu`](qu_curve_fit.qu) | Nonlinear least squares and general optimisation. |
| [`qu_bilinear_interp2.qu`](qu_bilinear_interp2.qu) | 2D grid interpolation. |
| [`qu_compressed_sensing.qu`](qu_compressed_sensing.qu) | Recovering a signal from far fewer samples than you'd expect. |
| [`qu_voronoi_diagram.qu`](qu_voronoi_diagram.qu) | Voronoi diagram of a small point set. |
| [`qu_random_walk.qu`](qu_random_walk.qu) | A discretised Wiener-process random walk. |

## Statistics, models and learning

| | |
|---|---|
| [`qu_corrcoef_heatmap.qu`](qu_corrcoef_heatmap.qu) | Correlation matrix and heatmap. |
| [`qu_ar_model_forecast.qu`](qu_ar_model_forecast.qu) | AR(2) fitting and forecast. |
| [`qu_pca_projection.qu`](qu_pca_projection.qu) | Fitting PCA and projecting onto components. |
| [`qu_classification_report.qu`](qu_classification_report.qu) | k-NN classifier with a full report. |
| [`qu_naive_bayes_classifier.qu`](qu_naive_bayes_classifier.qu) | Gaussian Naive Bayes, by hand. |
| [`qu_kalman_tracking.qu`](qu_kalman_tracking.qu) | Constant-velocity Kalman filter. |
| [`qu_particle_filter.qu`](qu_particle_filter.qu) | A GPU-dispatchable particle filter. |
| [`qu_hmm_healthy_fever.qu`](qu_hmm_healthy_fever.qu) | The classic 2-state Hidden Markov Model. |
| [`qu_markov_weather.qu`](qu_markov_weather.qu) | A two-state Markov chain. |
| [`qu_gridworld_rl.qu`](qu_gridworld_rl.qu) | Tabular reinforcement learning on a grid. |
| [`qu_sequential_xor.qu`](qu_sequential_xor.qu) | A tiny neural net learns XOR. |
| [`qu_lstm_forecast.qu`](qu_lstm_forecast.qu) | An LSTM cell unrolled over a real sequence. |
| [`qu_ml_report.qu`](qu_ml_report.qu) | Tables, an OLS model, and a generated report. |

## Figures and drawing

| | |
|---|---|
| [`qu_plot_types.qu`](qu_plot_types.qu) | Every kind of plot Qu draws, one panel each. |
| [`qu_line_styles.qu`](qu_line_styles.qu) | Every line style. |
| [`qu_markers.qu`](qu_markers.qu) | Every marker, at the size a figure actually uses. |
| [`qu_twin_axis_reference.qu`](qu_twin_axis_reference.qu) | A two-panel figure of the shape journals print. |
| [`qu_layers_schematic.qu`](qu_layers_schematic.qu) | Describe a symbol once, place it forty times. |
| [`qu_image_blobs.qu`](qu_image_blobs.qu) | Morphological cleanup and blob labelling. |
| [`qu_starry_night.qu`](qu_starry_night.qu) | An animated Starry Night, drawn entirely in Qu. |
| [`qu_sea_waves.qu`](qu_sea_waves.qu) | An animated sea, from the wave equation. |

## The language itself

| | |
|---|---|
| [`qu_multiple_dispatch.qu`](qu_multiple_dispatch.qu) | Functions overloaded on argument type. |
| [`qu_operator_overload.qu`](qu_operator_overload.qu) | Operators on your own record types. |
| [`qu_memoize_lazy.qu`](qu_memoize_lazy.qu) | `memoize function` and `lazy` bindings. |
| [`qu_watch_reactive.qu`](qu_watch_reactive.qu) | All three `watch` trigger forms. |
| [`qu_linked_list_and_graph.qu`](qu_linked_list_and_graph.qu) | A double-ended `linked_list`, used to build a graph. |
| [`qu_sandbox_profiling.qu`](qu_sandbox_profiling.qu) | `sandbox_mode`, `profiling_mode`, `profile_stats`. |

## Going faster

| | |
|---|---|
| [`qu_mutex_semaphore.qu`](qu_mutex_semaphore.qu) | Shared mutable state across real worker threads. |
| [`qu_distributed_pool.qu`](qu_distributed_pool.qu) | The local-worker half of the distributed pool. |
| [`qu_gpu_matmul.qu`](qu_gpu_matmul.qu) | Explicit GPU matmul, with an honest CPU comparison. |

## Files, formats and the network

| | |
|---|---|
| [`qu_file_extensions.qu`](qu_file_extensions.qu) | Three filesystem extensions in one flow. |
| [`qu_format_conversion.qu`](qu_format_conversion.qu) | The data-format conversion family. |
| [`qu_prm_loader.qu`](qu_prm_loader.qu) | A complete binary 3D-mesh parser, written entirely in Qu. |
| [`qu_streamfile_streamurl.qu`](qu_streamfile_streamurl.qu) | `StreamFile(path)` and `StreamURL(url)`. |
| [`qu_http_get.qu`](qu_http_get.qu) | A real HTTPS GET. Needs a network. |
| [`qu_tcp_echo.qu`](qu_tcp_echo.qu) | A client/server TCP round trip on loopback. |

## Apps and interaction

| | |
|---|---|
| [`qu_gui_controls.qu`](qu_gui_controls.qu) | A GUI declared by the script itself. |
| [`qu_interactive_sliders.qu`](qu_interactive_sliders.qu) | QuStudio's Interactive Mode. |
| [`qu_fluid_simulation.qu`](qu_fluid_simulation.qu) | 2D heat diffusion, explicit FTCS. Writes its final field as CSV so the NumPy version under `benchmarks/` can be diffed against it. |

---

## Two things worth knowing before you run a lot of these

**Some demos write files.** Several regenerate the `.svg` files kept beside
them, and a few write elsewhere in the repository. Running the whole catalog
leaves modified files behind — `git checkout catalog/` afterwards if you
didn't mean to keep them.

**Output is buffered when it isn't going to a terminal.** `qu run x.qu > log`
on a long, slow-converging script can leave `log` empty for a long time and
then fill it all at once. The script is not stuck — it prints its header
immediately, but redirected you may see nothing for close to a minute on
one of the heavier optimization demos.
