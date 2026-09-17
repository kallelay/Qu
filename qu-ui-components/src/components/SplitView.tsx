import React, { useState, useRef } from 'react';
import { motion } from 'framer-motion';
import { cn } from '../utils/cn';
import { glassStyle } from '../utils/glassStyle';
import { X, GripVertical, Minimize2, Maximize2 } from 'lucide-react';

export type SplitDirection = 'horizontal' | 'vertical';

export interface SplitViewProps {
  children: React.ReactNode[];
  direction?: SplitDirection;
  initialSizes?: number[];
  minSize?: number;
  className?: string;
  theme?: 'light' | 'dark' | 'system';
}

export const SplitView: React.FC<SplitViewProps> = ({
  children,
  direction = 'horizontal',
  initialSizes,
  minSize = 100,
  className,
  theme = 'system',
}) => {
  const [sizes, setSizes] = useState<number[]>(
    initialSizes || new Array(children.length).fill(100 / children.length)
  );
  const containerRef = useRef<HTMLDivElement>(null);
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);

  const handleMouseDown = (index: number) => {
    setDraggingIndex(index);
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (draggingIndex === null || !containerRef.current) return;

    const container = containerRef.current.getBoundingClientRect();
    const isHorizontal = direction === 'horizontal';
    const containerSize = isHorizontal ? container.width : container.height;
    const mousePosition = isHorizontal ? e.clientX - container.left : e.clientY - container.top;
    
    const newSizes = [...sizes];
    const totalSize = newSizes.reduce((a, b) => a + b, 0);
    
    const newSize1 = (mousePosition / containerSize) * totalSize;
    const newSize2 = totalSize - newSize1;

    if (newSize1 >= minSize && newSize2 >= minSize) {
      newSizes[draggingIndex] = newSize1;
      newSizes[draggingIndex + 1] = newSize2;
      setSizes(newSizes);
    }
  };

  const handleMouseUp = () => {
    setDraggingIndex(null);
  };

  const resolvedTheme = theme === 'system'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
    : theme;

  return (
    <motion.div
      ref={containerRef}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.2 }}
      className={cn(
        "flex overflow-hidden",
        direction === 'horizontal' ? "flex-row" : "flex-col",
        className
      )}
      onMouseMove={handleMouseMove}
      onMouseUp={handleMouseUp}
      onMouseLeave={handleMouseUp}
    >
      {children.map((child, index) => (
        <React.Fragment key={index}>
          <div
            style={{
              flex: sizes[index],
              minWidth: direction === 'horizontal' ? `${minSize}px` : undefined,
              minHeight: direction === 'vertical' ? `${minSize}px` : undefined,
            }}
            className="overflow-auto"
          >
            {child}
          </div>
          {index < children.length - 1 && (
            <div
              className={cn(
                "flex items-center justify-center cursor-col-resize transition-colors",
                direction === 'horizontal' 
                  ? "w-1 hover:w-2 hover:bg-[#3987e5]" 
                  : "h-1 hover:h-2 hover:bg-[#3987e5]",
                resolvedTheme === 'dark' ? "bg-[#1c2740]" : "bg-[#e1e0d9]"
              )}
              onMouseDown={() => handleMouseDown(index)}
            >
              <GripVertical size={12} className="opacity-0 hover:opacity-50" />
            </div>
          )}
        </React.Fragment>
      ))}
    </motion.div>
  );
};

export default SplitView;
