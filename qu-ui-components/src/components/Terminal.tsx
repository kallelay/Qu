import React, { useState, useRef, useEffect } from 'react';
import { motion } from 'framer-motion';
import { cn } from '../utils/cn';
import { useIDEStore } from '../store/ideStore';
import { X, ChevronUp, ChevronDown } from 'lucide-react';

export interface TerminalProps {}

export const Terminal: React.FC<TerminalProps> = () => {
  const { terminalOutput, clearTerminal, toggleTerminal } = useIDEStore();
  const [input, setInput] = useState('');
  const terminalRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.scrollTop = terminalRef.current.scrollHeight;
    }
  }, [terminalOutput]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && input.trim()) {
      console.log('Terminal command:', input);
      setInput('');
    }
  };

  return (
    <div className="h-full flex flex-col bg-[#0e1524]">
      <div
        className="flex items-center justify-between px-3 py-1.5 border-b border-[#1c2740]"
        style={{ background: 'linear-gradient(180deg, #202c42, #141d30)' }}
      >
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold text-[#6b7a94]">TERMINAL</span>
        </div>
        <div className="flex items-center gap-1">
          <button className="p-1 hover:bg-[#1a2540] rounded text-[#6b7a94]">
            <ChevronUp size={14} />
          </button>
          <button className="p-1 hover:bg-[#1a2540] rounded text-[#6b7a94]">
            <ChevronDown size={14} />
          </button>
          <button 
            onClick={clearTerminal}
            className="p-1 hover:bg-[#1a2540] rounded text-[#6b7a94]"
          >
            <X size={14} />
          </button>
        </div>
      </div>

      {/* Terminal Output */}
      <div 
        ref={terminalRef}
        className="flex-1 overflow-y-auto p-3 font-mono text-sm"
      >
        {terminalOutput.map((line, i) => (
          <div key={i} className="text-[#dfe7f2] py-0.5">
            {line}
          </div>
        ))}
        {terminalOutput.length === 0 && (
          <div className="text-[#6b7a94] italic">
            Qu REPL ready. Type commands below...
          </div>
        )}
      </div>

      {/* Terminal Input */}
      <div className="flex items-center border-t border-[#1c2740] p-2">
        <span className="text-[#6cb6ff] mr-2">{'>'}</span>
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          className="flex-1 bg-transparent text-[#dfe7f2] outline-none font-mono text-sm"
          placeholder="Enter Qu command..."
        />
      </div>
    </div>
  );
};
