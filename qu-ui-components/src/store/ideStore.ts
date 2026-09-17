import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';

export interface FileTab {
  id: string;
  name: string;
  /** Absolute on-disk path, or `null` for a buffer never yet saved (a new
   *  untitled tab, or a hardcoded template/catalog entry opened before its
   *  first "Save As"). */
  path: string | null;
  content: string;
  isDirty: boolean;
  language: string;
}

export interface IDEState {
  // Files & Tabs
  activeFileId: string | null;
  tabs: FileTab[];
  openFiles: string[];

  // Editor
  editorContent: string;
  cursorPosition: { line: number; column: number };

  // Terminal
  terminalOutput: string[];
  terminalHistory: string[];

  // Variables
  variables: Record<string, any>;

  // Plots
  plots: any[];

  // UI State
  sidebarOpen: boolean;
  terminalOpen: boolean;
  variablesOpen: boolean;
  plotsOpen: boolean;

  // Actions
  setActiveFile: (id: string | null) => void;
  openFile: (file: FileTab) => void;
  closeFile: (id: string) => void;
  updateFileContent: (id: string, content: string) => void;
  /** Marks a tab as saved (isDirty -> false), optionally recording the
   *  on-disk path/name it was just saved to for the first time (a "Save
   *  As" on a previously-untitled tab). When `path` is given, the tab's
   *  `id` is promoted to that path too, so a later file-tree/catalog open
   *  of the same file focuses this tab instead of creating a duplicate. */
  markFileSaved: (id: string, savedAs?: { path: string; name: string }) => void;
  /** Creates a fresh, empty, never-saved tab (an "untitled-N.qu" buffer)
   *  and makes it active. Returns the new tab's id. */
  newUntitledFile: () => string;
  /** Moves `activeFileId` to the next (`direction: 1`) or previous
   *  (`direction: -1`) tab, wrapping around. No-op with 0 or 1 tabs. */
  cycleActiveFile: (direction: 1 | -1) => void;
  addTerminalOutput: (output: string) => void;
  clearTerminal: () => void;
  setVariables: (vars: Record<string, any>) => void;
  addPlot: (plot: any) => void;
  clearPlots: () => void;
  toggleSidebar: () => void;
  toggleTerminal: () => void;
  toggleVariables: () => void;
  togglePlots: () => void;
}

export const useIDEStore = create<IDEState>()(
  subscribeWithSelector((set, get) => ({
    // Initial State
    activeFileId: null,
    tabs: [],
    openFiles: [],
    editorContent: '',
    cursorPosition: { line: 1, column: 1 },
    terminalOutput: [],
    terminalHistory: [],
    variables: {},
    plots: [],
    sidebarOpen: true,
    terminalOpen: true,
    variablesOpen: false,
    plotsOpen: false,

    // Actions
    setActiveFile: (id) => set((state) => ({
      activeFileId: id,
      editorContent: id ? state.tabs.find(t => t.id === id)?.content ?? state.editorContent : '',
    })),

    openFile: (file) => set((state) => {
      const exists = state.tabs.find(t => t.id === file.id);
      if (exists) {
        // Already open: just focus it, keeping whatever content/dirty
        // state it already has -- re-opening the same file/template must
        // never clobber in-progress edits in its existing tab.
        return { activeFileId: file.id, editorContent: exists.content };
      }
      return {
        tabs: [...state.tabs, file],
        openFiles: [...state.openFiles, file.id],
        activeFileId: file.id,
        editorContent: file.content,
      };
    }),

    closeFile: (id) => set((state) => {
      const closedIndex = state.tabs.findIndex(t => t.id === id);
      const newTabs = state.tabs.filter(t => t.id !== id);
      const newOpenFiles = state.openFiles.filter(fid => fid !== id);
      let newActiveId = state.activeFileId;
      if (state.activeFileId === id) {
        // Prefer the tab that was to the right of the closed one (matches
        // most editors' convention), falling back to the new last tab.
        newActiveId = newTabs.length > 0
          ? (newTabs[closedIndex]?.id ?? newTabs[newTabs.length - 1].id)
          : null;
      }

      return {
        tabs: newTabs,
        openFiles: newOpenFiles,
        activeFileId: newActiveId,
        editorContent: newActiveId ? newTabs.find(t => t.id === newActiveId)?.content || '' : '',
      };
    }),

    updateFileContent: (id, content) => set((state) => ({
      tabs: state.tabs.map(t => t.id === id ? { ...t, content, isDirty: true } : t),
      editorContent: state.activeFileId === id ? content : state.editorContent,
    })),

    markFileSaved: (id, savedAs) => set((state) => {
      const newId = savedAs?.path ?? id;
      return {
        tabs: state.tabs.map(t => t.id === id
          ? { ...t, isDirty: false, id: newId, path: savedAs?.path ?? t.path, name: savedAs?.name ?? t.name }
          : t),
        openFiles: state.openFiles.map(fid => fid === id ? newId : fid),
        activeFileId: state.activeFileId === id ? newId : state.activeFileId,
      };
    }),

    newUntitledFile: () => {
      const state = get();
      const existingIds = new Set(state.tabs.map(t => t.id));
      let n = 1;
      while (existingIds.has(`untitled-${n}`)) n++;
      const id = `untitled-${n}`;
      const tab: FileTab = { id, name: `untitled-${n}.qu`, path: null, content: '', isDirty: false, language: 'qu' };
      set({
        tabs: [...state.tabs, tab],
        openFiles: [...state.openFiles, id],
        activeFileId: id,
        editorContent: '',
      });
      return id;
    },

    cycleActiveFile: (direction) => set((state) => {
      if (state.tabs.length < 2) return {};
      const idx = state.tabs.findIndex(t => t.id === state.activeFileId);
      const nextIdx = ((idx === -1 ? 0 : idx) + direction + state.tabs.length) % state.tabs.length;
      const next = state.tabs[nextIdx];
      return { activeFileId: next.id, editorContent: next.content };
    }),

    addTerminalOutput: (output) => set((state) => ({
      terminalOutput: [...state.terminalOutput, output],
      terminalHistory: [...state.terminalHistory, output],
    })),

    clearTerminal: () => set({ terminalOutput: [] }),

    setVariables: (vars) => set({ variables: vars }),

    addPlot: (plot) => set((state) => ({
      plots: [...state.plots, plot],
    })),

    clearPlots: () => set({ plots: [] }),

    toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
    toggleTerminal: () => set((state) => ({ terminalOpen: !state.terminalOpen })),
    toggleVariables: () => set((state) => ({ variablesOpen: !state.variablesOpen })),
    togglePlots: () => set((state) => ({ plotsOpen: !state.plotsOpen })),
  }))
);
