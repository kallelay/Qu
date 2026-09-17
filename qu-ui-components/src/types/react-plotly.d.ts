// `react-plotly.js` ships no bundled types and `@types/react-plotly.js` is
// not installed in this workspace. This minimal ambient declaration is
// intentionally loose (props typed as `any`) -- it only exists to satisfy
// `tsc` for the default-exported <Plot /> component used in PlotViewer.tsx.
declare module 'react-plotly.js' {
  import * as React from 'react';

  interface PlotParams {
    data: any[];
    layout?: any;
    config?: any;
    frames?: any[];
    style?: React.CSSProperties;
    className?: string;
    useResizeHandler?: boolean;
    onInitialized?: (figure: any, graphDiv: HTMLElement) => void;
    onUpdate?: (figure: any, graphDiv: HTMLElement) => void;
    onPurge?: (figure: any, graphDiv: HTMLElement) => void;
    onError?: (err: Error) => void;
    [key: string]: any;
  }

  type PlotComponent = React.ForwardRefExoticComponent<
    PlotParams & React.RefAttributes<any>
  >;

  const Plot: PlotComponent;

  export default Plot;
}

// Use the same bundled Plotly instance as react-plotly.js for imperative APIs.
declare module 'plotly.js/dist/plotly' {
  import * as Plotly from 'plotly.js';
  export default Plotly;
}
