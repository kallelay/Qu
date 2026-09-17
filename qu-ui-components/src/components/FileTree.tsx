import React, { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { cn } from '../utils/cn';
import {
  ChevronRight,
  ChevronDown,
  File,
  Folder,
  FolderOpen,
  FileCode,
  FileJson,
  FileText,
  Plus,
  Trash2,
  Edit2,
} from 'lucide-react';
import './inspector.css';

export interface FileNode {
  id: string;
  name: string;
  type: 'file' | 'folder';
  path: string;
  children?: FileNode[];
  isOpen?: boolean;
  icon?: string;
}

export interface FileTreeProps {
  files?: FileNode[];
  selectedFile?: string;
  onSelect?: (file: FileNode) => void;
  onOpen?: (file: FileNode) => void;
  /** Renaming is the HOST'S job -- it owns the prompt and the write. The
   *  signature used to be `(file, newName)`, a second argument nothing
   *  in the tree could ever produce, which is part of why no call site
   *  was ever wired up. */
  onRename?: (file: FileNode) => void;
  onDelete?: (file: FileNode) => void;
  onNewFile?: (path: string) => void;
  onNewFolder?: (path: string) => void;
  className?: string;
  theme?: 'light' | 'dark' | 'system';
}

/** Row actions are rendered ONLY for the handlers the host actually
 *  supplies. They used to be three unconditional buttons with no
 *  `onClick` at all: hovering any file in the Studio's Files tab revealed
 *  add / rename / delete, and clicking any of them did nothing, silently.
 *  `onRename`/`onDelete`/`onNewFile`/`onNewFolder` were declared on the
 *  props type and never read (TypeScript was reporting all four as
 *  unused), which is the same fact stated from the other end. Nothing is
 *  removed here -- a host that passes a handler gets a working button;
 *  the Studio passes none, so it stops advertising three actions it
 *  cannot perform. */
const FileTreeItem: React.FC<{
  node: FileNode;
  depth: number;
  selectedId?: string;
  onSelect: (node: FileNode) => void;
  onRename?: (node: FileNode) => void;
  onDelete?: (node: FileNode) => void;
  onNewFile?: (path: string) => void;
}> = ({ node, depth, selectedId, onSelect, onRename, onDelete, onNewFile }) => {
  const [isHovered, setIsHovered] = useState(false);
  const [isOpen, setIsOpen] = useState(node.isOpen || false);
  const hasChildren = node.children && node.children.length > 0;

  const getFileIcon = (name: string) => {
    if (name.endsWith('.qu')) return <FileCode size={16} />;
    if (name.endsWith('.json')) return <FileJson size={16} />;
    if (name.endsWith('.md') || name.endsWith('.txt')) return <FileText size={16} />;
    return <File size={16} />;
  };

  const hasRowActions = !!(onRename || onDelete || (onNewFile && node.type === 'folder'));

  return (
    <div>
      <div
        className="qu-file-row flex items-center gap-1.5 px-2 py-1 cursor-pointer rounded-md"
        data-selected={selectedId === node.id || undefined}
        style={{ paddingLeft: `${depth * 12 + 8}px` }}
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        onClick={() => {
          if (hasChildren) setIsOpen(!isOpen);
          onSelect(node);
        }}
      >
        {hasChildren ? (
          isOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />
        ) : (
          <span className="w-3.5" />
        )}

        {node.type === 'folder' ? (
          isOpen
            ? <FolderOpen size={15} style={{ color: 'var(--qu-warning)' }} />
            : <Folder size={15} style={{ color: 'var(--qu-warning)' }} />
        ) : (
          <span style={{ color: 'var(--qu-muted)' }}>{getFileIcon(node.name)}</span>
        )}

        <span className="text-[13px] truncate flex-1">{node.name}</span>

        <AnimatePresence>
          {isHovered && hasRowActions && (
            <motion.div
              initial={{ opacity: 0, x: 6 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 6 }}
              className="flex items-center gap-0.5"
              onClick={(e) => e.stopPropagation()}
            >
              {onNewFile && node.type === 'folder' && (
                <button className="qu-file-action" title={`New file in ${node.name}`} onClick={() => onNewFile(node.path)}>
                  <Plus size={12} />
                </button>
              )}
              {onRename && (
                <button className="qu-file-action" title={`Rename ${node.name}`} onClick={() => onRename(node)}>
                  <Edit2 size={12} />
                </button>
              )}
              {onDelete && (
                <button className="qu-file-action is-danger" title={`Delete ${node.name}`} onClick={() => onDelete(node)}>
                  <Trash2 size={12} />
                </button>
              )}
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      <AnimatePresence>
        {isOpen && hasChildren && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.15 }}
          >
            {node.children!.map((child) => (
              <FileTreeItem
                key={child.id}
                node={child}
                depth={depth + 1}
                selectedId={selectedId}
                onSelect={onSelect}
                onRename={onRename}
                onDelete={onDelete}
                onNewFile={onNewFile}
              />
            ))}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
};

export const FileTree: React.FC<FileTreeProps> = ({
  files = [],
  selectedFile,
  onSelect,
  onRename,
  onDelete,
  onNewFile,
  className,
}) => {
  return (
    // The tree used to be a translucent, blurred, rounded "glass" card
    // with its own border -- a floating panel nested inside the sidebar
    // panel it already fills, so the Files tab showed a card inside a
    // card. Nothing else in the app frames a list this way. It is a list;
    // it sits in the panel it was given.
    <motion.div
      initial={{ opacity: 0, x: -8 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.18 }}
      className={cn('h-full overflow-y-auto', className)}
    >
      <div className="qu-file-heading">
        Explorer
      </div>

      <div className="pb-2">
        {files.map((file) => (
          <FileTreeItem
            key={file.id}
            node={file}
            depth={0}
            selectedId={selectedFile}
            onSelect={onSelect || (() => {})}
            onRename={onRename}
            onDelete={onDelete}
            onNewFile={onNewFile}
          />
        ))}
      </div>
    </motion.div>
  );
};

export default FileTree;
