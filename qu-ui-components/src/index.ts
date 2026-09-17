/**
 * @qu/ui-components
 * 
 * Shared UI component library for QuStudio & MattyStudio
 * Modern, beautiful, high-performance IDE components
 */

// Core Components
export { CodeEditor } from './components/CodeEditor';
export type { CodeEditorProps, Language, RunCellPayload } from './components/CodeEditor';

export { PlotViewer } from './components/PlotViewer';
export type { PlotViewerProps, PlotType, PlotData } from './components/PlotViewer';

export { FigureViewer } from './components/FigureViewer';
export type { FigureViewerProps } from './components/FigureViewer';

export { GuiDesigner } from './components/GuiDesigner';
export type { GuiDesignerProps } from './components/GuiDesigner';
// § Design/Code/Run split (2026-09-16): the host app assembles
// form.design.qu (GuiDesigner's own output) + form.code.qu (the user's
// own file) into form.gen.qu using these, so they need to cross the
// package boundary -- see guiDesign.ts's own doc comments for each.
export { requiredHandlerNames, reconcileCodeQu, mergeGenScript } from './utils/guiDesign';

export { VersionHistoryPanel } from './components/VersionHistoryPanel';
export type { VersionHistoryPanelProps, FileVersion } from './components/VersionHistoryPanel';

export { HelpBrowser } from './components/HelpBrowser';
export type { HelpBrowserProps, BuiltinDoc } from './components/HelpBrowser';

export { FileTree } from './components/FileTree';
export type { FileTreeProps, FileNode } from './components/FileTree';

export { Terminal } from './components/Terminal';
export type { TerminalProps } from './components/Terminal';

export { StatusBar } from './components/StatusBar';
export type { StatusBarProps, StatusType } from './components/StatusBar';

export { TabBar } from './components/TabBar';
export type { TabBarProps, Tab } from './components/TabBar';

export { SplitView } from './components/SplitView';
export type { SplitViewProps, SplitDirection } from './components/SplitView';

export { CommandPalette } from './components/CommandPalette';
export type { CommandPaletteProps, Command } from './components/CommandPalette';

export { VariableExplorer } from './components/VariableExplorer';
export type { VariableExplorerProps, Variable } from './components/VariableExplorer';

export { Mascot } from './components/Mascot';
export type { MascotProps, MascotMessage } from './components/Mascot';

export { AiDiffModal } from './components/AiDiffModal';
export type { AiDiffModalProps } from './components/AiDiffModal';

// Layout Components
export { IDEShell } from './components/IDESShell';
export type { IDEShellProps } from './components/IDESShell';

export { Panel } from './components/Panel';
export type { PanelProps } from './components/Panel';

// Theme & State
export { useTheme, ThemeProvider, ThemeToggle } from './hooks/useTheme';
export { useIDEStore } from './store/ideStore';
export type { IDEState, FileTab } from './store/ideStore';

// Utilities
export { cn } from './utils/cn';
export { glassStyle } from './utils/glassStyle';
export { parseCells, findCellAtLine, getPrefixSource } from './utils/cells';
export type { QuCell } from './utils/cells';
export { classifyStat, confirmChange, hasLocalEdits } from './utils/diskWatch';
export type { DiskStat, DiskBaseline, StatVerdict, DiskChange } from './utils/diskWatch';
export { columnSelectionRanges } from './utils/columnSelect';
export type { ColumnRange, ColumnPoint } from './utils/columnSelect';
export type { NumericVariable } from './utils/variableData';
export { serialFigures, parseIndexFilter, IMPEDANCE_VIEWS } from './utils/serialPlots';
export { FIGURE_FORMATS, svgToPng, buildFigureFiles, engineSaveSnippet } from './utils/figureExport';
export type { FigureFormat } from './utils/figureExport';
export { assignedNamesInOrder, evalConstExpr, readFigureTarget, setFigureTarget, insertPlotCode } from './utils/scriptVars';
export type { FigureTarget } from './utils/scriptVars';
export type { SerialMode, SerialFrame, ImpedanceView } from './utils/serialPlots';
