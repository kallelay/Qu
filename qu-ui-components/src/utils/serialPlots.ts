import type { PlotData } from '../components/PlotViewer';

export type SerialMode = 'signal' | 'impedance';
export interface SerialFrame { id: number; rows: number[][] }
export type ImpedanceView = 'nyquist' | 'magnitude' | 'phase' | 'real' | 'imaginary' | 'nyquist-bode' | 'nyquist-components';
export const IMPEDANCE_VIEWS: { value: ImpedanceView; label: string }[] = [
  { value: 'nyquist', label: 'Nyquist' },
  { value: 'magnitude', label: 'Bode · magnitude' },
  { value: 'phase', label: 'Bode · phase' },
  { value: 'real', label: 'Real impedance' },
  { value: 'imaginary', label: 'Imaginary impedance' },
  { value: 'nyquist-bode', label: 'Nyquist + Bode' },
  { value: 'nyquist-components', label: 'Nyquist + Re / Im' },
];

/** Store intervals, never expand a user-supplied range into an enormous array. */
export function parseIndexFilter(input: string): (index: number) => boolean {
  const text = input.trim();
  if (!text) return () => true;
  const exclude = /^skip:/i.test(text);
  const body = exclude ? text.slice(5).trim() : text;
  if (!body) throw new Error('Add indices after skip:, for example skip:3,7.');
  const intervals = body.split(',').map(part => {
    const match = /^(\d+)\s*(?:-\s*(\d+))?$/.exec(part.trim());
    if (!match) throw new Error('Use indices or ranges: 0-5,8,10-19 or skip:3,7.');
    const first = Number(match[1]); const last = Number(match[2] ?? match[1]);
    if (!Number.isSafeInteger(first) || !Number.isSafeInteger(last) || first > last) {
      throw new Error('Ranges must use ascending, nonnegative whole indices.');
    }
    return [first, last];
  });
  return index => exclude !== intervals.some(([first, last]) => index >= first && index <= last);
}

export interface SerialFigure { key: string; title: string; xlabel: string; ylabel: string; data: PlotData[]; equalAspect?: boolean }
export function serialFigures(mode: SerialMode, view: ImpedanceView, frames: SerialFrame[], fading: boolean, filter: (index: number) => boolean): SerialFigure[] {
  const selected = frames.slice(fading ? -10 : -1);
  const traces = (kind: string, column: number, name: string, color: string, accumulated = false): PlotData[] => selected.map((frame, i) => {
    const age = selected.length - i - 1;
    const rows = mode === 'signal' ? frame.rows : frame.rows.filter(row => filter(row[0]));
    const real = (row: number[]) => row[1] * Math.cos(row[2] * Math.PI / 180);
    const imaginary = (row: number[]) => row[1] * Math.sin(row[2] * Math.PI / 180);
    return {
      uid: `${kind}-${column}-age-${age}`,
      name, type: 'scatter', mode: mode === 'signal' ? 'lines' : 'lines+markers',
      x: rows.map((row, index) => mode === 'signal' ? index : kind === 'nyquist' ? real(row) : row[0]),
      y: rows.map(row => kind === 'nyquist' ? -imaginary(row) : kind === 'real' ? real(row) : kind === 'imaginary' ? imaginary(row) : row[column]),
      opacity: selected.length === 1 ? 1 : 0.15 + 0.85 * i / (selected.length - 1),
      showlegend: age === 0, legendgroup: name,
      line: { color, width: age === 0 ? 2 : 1, dash: accumulated ? 'dash' : 'solid' },
      marker: { color, size: age === 0 ? 5 : 3 },
    };
  });
  const teal = '#2a9d8f'; const coral = '#e78462';
  if (mode === 'signal') return [
    { key: 'adc1', title: 'ADC 1 · voltage channel', xlabel: 'Sample index', ylabel: 'Raw ADC', data: traces('signal', 0, 'ADC 1', teal) },
    { key: 'adc2', title: 'ADC 2 · current channel', xlabel: 'Sample index', ylabel: 'Raw ADC', data: traces('signal', 1, 'ADC 2', coral) },
  ];
  const figure = (kind: string): SerialFigure => {
    const title = kind === 'nyquist' ? 'Nyquist' : kind === 'magnitude' ? 'Bode · magnitude' : kind === 'phase' ? 'Bode · phase' : kind === 'real' ? 'Real impedance' : 'Imaginary impedance';
    const ylabel = kind === 'nyquist' ? '−Im(Z) [Ω]' : kind === 'magnitude' ? '|Z| [Ω]' : kind === 'phase' ? 'Phase [°]' : kind === 'real' ? 'Re(Z) [Ω]' : 'Im(Z) [Ω]';
    const column = kind === 'phase' ? 2 : 1;
    return { key: kind, title, xlabel: kind === 'nyquist' ? 'Re(Z) [Ω]' : 'Frequency index', ylabel,
      equalAspect: kind === 'nyquist',
      data: [...traces(kind, column, 'Instantaneous', teal), ...(['magnitude', 'phase'].includes(kind) ? traces(kind, column + 2, 'Accumulated', coral, true) : [])],
    };
  };
  return (view === 'nyquist-bode' ? ['nyquist', 'magnitude', 'phase'] : view === 'nyquist-components' ? ['nyquist', 'real', 'imaginary'] : [view]).map(figure);
}
