import React, { useState } from 'react';
import { motion } from 'framer-motion';
import { cn } from '../utils/cn';
import { glassStyle } from '../utils/glassStyle';
import { useIDEStore } from '../store/ideStore';
import { CodeEditor } from './CodeEditor';
import { FileTree } from './FileTree';
import { TabBar } from './TabBar';
import { StatusBar } from './StatusBar';
import { Terminal } from './Terminal';
import { PlotViewer } from './PlotViewer';
import { 
  PanelLeft, 
  PanelRight, 
  Terminal as TerminalIcon, 
  Play,
  Save,
  Settings,
  Search
} from 'lucide-react';

export interface IDEShellProps {
  children?: React.ReactNode;
  className?: string;
  theme?: 'light' | 'dark' | 'system';
}

export const IDEShell: React.FC<IDEShellProps> = ({
  children,
  className,
  theme = 'system',
}) => {
  const {
    sidebarOpen,
    terminalOpen,
    plotsOpen,
    tabs,
    activeFileId,
    editorContent,
    toggleSidebar,
    toggleTerminal,
    togglePlots,
  } = useIDEStore();

  const [terminalHeight, setTerminalHeight] = useState(200);

  const resolvedTheme = theme === 'system'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    : theme;

  return (
    <div className={cn(
      "h-screen w-screen overflow-hidden flex flex-col",
      resolvedTheme === 'dark' ? "bg-[#0d0d0d]" : "bg-[#f9f9f7]",
      className
    )}>
      {/* Top Bar */}
      <motion.div
        initial={{ y: -20, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        className={cn(
          "h-12 flex items-center px-4 border-b",
          resolvedTheme === 'dark' 
            ? "bg-[#161615]/80 border-[#2c2c2a]" 
            : "bg-[#ffffff]/80 border-[#e1e0d9]",
          glassStyle
        )}
      >
        <button
          onClick={toggleSidebar}
          className={cn(
            "p-2 rounded-lg transition-colors mr-2",
            resolvedTheme === 'dark'
              ? "hover:bg-[#2c2c2a] text-[#c3c2b7]"
              : "hover:bg-[#e1e0d9] text-[#52514e]"
          )}
        >
          <PanelLeft size={18} />
        </button>
        
        <div className="flex items-center gap-2 mr-4">
          <span className={cn(
            "font-bold text-lg",
            resolvedTheme === 'dark' ? "text-[#3987e5]" : "text-[#2a78d6]"
          )}>
            Qu
          </span>
          <span className={cn(
            "text-xs px-2 py-0.5 rounded-full border",
            resolvedTheme === 'dark'
              ? "bg-[#2c2c2a] border-[#383835] text-[#898781]"
              : "bg-[#e1e0d9] border-[#c3c2b7] text-[#898781]"
          )}>
            Studio
          </span>
        </div>

        <div className="flex items-center gap-2">
          <button
            className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-lg font-medium transition-all",
              "hover:scale-105 active:scale-95",
              resolvedTheme === 'dark'
                ? "bg-[#2a78d6] text-white hover:bg-[#3987e5]"
                : "bg-[#2a78d6] text-white hover:bg-[#3987e5]"
            )}
          >
            <Play size={14} />
            Run
          </button>
          
          <button
            className={cn(
              "p-2 rounded-lg transition-colors",
              resolvedTheme === 'dark'
                ? "hover:bg-[#2c2c2a] text-[#c3c2b7]"
                : "hover:bg-[#e1e0d9] text-[#52514e]"
            )}
          >
            <Save size={18} />
          </button>
          
          <button
            className={cn(
              "p-2 rounded-lg transition-colors",
              resolvedTheme === 'dark'
                ? "hover:bg-[#2c2c2a] text-[#c3c2b7]"
                : "hover:bg-[#e1e0d9] text-[#52514e]"
            )}
          >
            <Search size={18} />
          </button>
          
          <button
            className={cn(
              "p-2 rounded-lg transition-colors",
              resolvedTheme === 'dark'
                ? "hover:bg-[#2c2c2a] text-[#c3c2b7]"
                : "hover:bg-[#e1e0d9] text-[#52514e]"
            )}
          >
            <Settings size={18} />
          </button>
        </div>
      </motion.div>

      {/* Main Content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Sidebar */}
        <motion.div
          initial={{ x: -200, opacity: 0 }}
          animate={{ 
            x: sidebarOpen ? 0 : -280, 
            opacity: sidebarOpen ? 1 : 0 
          }}
          transition={{ duration: 0.2 }}
          className={cn(
            "w-64 flex-shrink-0 border-r overflow-hidden",
            resolvedTheme === 'dark' 
              ? "bg-[#161615]/80 border-[#2c2c2a]" 
              : "bg-[#ffffff]/80 border-[#e1e0d9]",
            glassStyle
          )}
        >
          <FileTree
            files={[
              {
                id: '1',
                name: 'examples',
                type: 'folder',
                path: '/examples',
                children: [
                  { id: '2', name: 'multisine.qu', type: 'file', path: '/examples/multisine.qu' },
                  { id: '3', name: 'fft_analysis.qu', type: 'file', path: '/examples/fft_analysis.qu' },
                ],
              },
              { id: '4', name: 'untitled.qu', type: 'file', path: '/untitled.qu' },
            ]}
            theme={theme}
          />
        </motion.div>

        {/* Editor Area */}
        <div className="flex-1 flex flex-col min-w-0">
          {/* Tabs */}
          <TabBar
            tabs={tabs.map(t => ({ id: t.id, name: t.name, isDirty: t.isDirty }))}
            activeTabId={activeFileId || ''}
          />

          {/* Editor */}
          <div className="flex-1 overflow-hidden">
            <CodeEditor
              value={editorContent}
              language="qu"
              theme={theme}
            />
          </div>

          {/* Terminal Panel */}
          {terminalOpen && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: terminalHeight, opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              className={cn(
                "border-t overflow-hidden",
                resolvedTheme === 'dark' 
                  ? "bg-[#0e1524] border-[#1c2740]" 
                  : "bg-[#f5f5f5] border-[#e1e0d9]"
              )}
              style={{ height: terminalHeight }}
            >
              <Terminal />
            </motion.div>
          )}
        </div>

        {/* Right Panel - Plots */}
        {plotsOpen && (
          <motion.div
            initial={{ x: 300, opacity: 0 }}
            animate={{ x: 0, opacity: 1 }}
            exit={{ x: 300, opacity: 0 }}
            className={cn(
              "w-96 flex-shrink-0 border-l overflow-y-auto",
              resolvedTheme === 'dark' 
                ? "bg-[#161615]/80 border-[#2c2c2a]" 
                : "bg-[#ffffff]/80 border-[#e1e0d9]",
              glassStyle
            )}
          >
            <div className="p-4">
              <h3 className={cn(
                "text-sm font-semibold mb-3",
                resolvedTheme === 'dark' ? "text-[#c3c2b7]" : "text-[#52514e]"
              )}>
                Plots
              </h3>
              {/* Plot viewers would go here */}
            </div>
          </motion.div>
        )}
      </div>

      {/* Status Bar */}
      <StatusBar />
    </div>
  );
};

export default IDEShell;
