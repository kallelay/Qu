import React from 'react';
import { cn } from '../utils/cn';
// Shares the app's token set and the `.qu-filetab` rules. Imported here
// rather than relied on being pulled in by whichever other component
// happens to be bundled alongside.
import './inspector.css';

export interface Tab {
  id: string;
  name: string;
  isDirty?: boolean;
}

export interface TabBarProps {
  tabs: Tab[];
  activeTabId: string;
  onTabClick?: (id: string) => void;
  onTabClose?: (id: string) => void;
}

export const TabBar: React.FC<TabBarProps> = ({
  tabs,
  activeTabId,
  onTabClick,
  onTabClose,
}) => {
  return (
    // Every colour in this strip used to be a literal from the old navy
    // palette -- including the `#16203a -> #0e1524` gradient -- with no
    // theme branch at all, so the file tabs were a dark blue band across
    // the top of a white editor in light mode. On tokens it simply
    // follows the theme, and the active tab now shares the top bar's
    // accent-underline language instead of inventing a third "this one is
    // selected" treatment for the same window.
    <div
      className="flex items-center border-b flex-shrink-0 overflow-x-auto"
      style={{ background: 'var(--qu-shell-panel)', borderColor: 'var(--qu-border)' }}
      role="tablist"
      aria-label="Open files"
    >
      {tabs.map((tab) => (
        <div
          key={tab.id}
          role="tab"
          aria-selected={activeTabId === tab.id}
          className={cn(
            // `group` so the close button below can react to hovering
            // anywhere on the tab, not just the few px it occupies --
            // it used to only reveal itself once the pointer was already
            // on top of it, which nothing on screen hinted at.
            "qu-filetab group flex items-center gap-2 px-3.5 py-2 text-[13px] cursor-pointer border-r whitespace-nowrap",
          )}
          onClick={() => onTabClick?.(tab.id)}
        >
          <span>{tab.name}</span>
          {/* The dirty dot used to sit BETWEEN the name and the close
              button and stayed put when the close button faded in, so
              the row reflowed under the pointer on hover. It now shares
              one fixed-width slot with the close button -- the dot when
              at rest, the X on hover -- which is the arrangement every
              editor with this affordance converged on, and which also
              stops a saved tab and an unsaved one being different
              widths. */}
          <span className="relative w-3.5 h-3.5 flex items-center justify-center flex-shrink-0">
            {tab.isDirty && (
              <span
                className="qu-filetab-dot w-2 h-2 rounded-full"
                style={{ background: 'var(--qu-warning)' }}
                aria-label="Unsaved changes"
              />
            )}
            <button
              onClick={(e) => {
                e.stopPropagation();
                onTabClose?.(tab.id);
              }}
              aria-label={`Close ${tab.name}`}
              className="qu-filetab-close absolute inset-0 flex items-center justify-center rounded"
            >
              <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
                <path d="M2.5 2.5L9.5 9.5M9.5 2.5L2.5 9.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
              </svg>
            </button>
          </span>
        </div>
      ))}
    </div>
  );
};
