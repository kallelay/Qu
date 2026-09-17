import React from 'react';
import { cn } from '../utils/cn';

export interface PanelProps {
  children?: React.ReactNode;
  className?: string;
  title?: string;
}

/**
 * Generic titled container used for the sidebar/side-panel regions of the
 * IDE shell (variable explorer, plot list, etc.) — deliberately minimal,
 * not yet wired to any specific panel's own layout needs.
 */
export const Panel: React.FC<PanelProps> = ({ children, className, title }) => {
  return (
    <div className={cn('flex flex-col h-full', className)}>
      {title && (
        <div className="px-3 py-2 text-xs font-semibold uppercase tracking-wide text-[#898781] border-b border-[#2c2c2a]">
          {title}
        </div>
      )}
      <div className="flex-1 overflow-auto">{children}</div>
    </div>
  );
};

export default Panel;
