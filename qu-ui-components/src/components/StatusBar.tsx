import React from 'react';
import { cn } from '../utils/cn';
import { useIDEStore } from '../store/ideStore';

export type StatusType = 'info' | 'error' | 'success' | 'warning';

export interface StatusBarProps {}

export const StatusBar: React.FC<StatusBarProps> = () => {
  const { cursorPosition } = useIDEStore();

  return (
    <div className="h-6 flex items-center px-3 text-xs border-t bg-[#0e1524] border-[#1c2740] text-[#6b7a94]">
      <span className="mr-4">Ln {cursorPosition.line}, Col {cursorPosition.column}</span>
      <span className="mr-4">UTF-8</span>
      <span className="mr-4">Qu</span>
      <span className="ml-auto">Prettier</span>
      <span className="ml-4">LF</span>
    </div>
  );
};
