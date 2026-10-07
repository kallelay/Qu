import React, { useEffect, useMemo, useState } from 'react';
import { ExternalLink, FileText } from 'lucide-react';
import './viewers.css';

export interface PdfViewerProps {
  /** The file's bytes (already size-checked by the host). */
  data: Uint8Array;
  name: string;
  path?: string | null;
  /** Hands the file to the OS default viewer. */
  onOpenExternal?: () => void;
  /** Test/override hook: force the "no native PDF support" fallback. */
  forceFallback?: boolean;
}

/** Whether this webview can show a PDF inline. Chromium (WebView2) and
 *  WebKit on macOS report `navigator.pdfViewerEnabled`; WebKitGTK on Linux
 *  does not. Anything unknown is treated as unsupported -- the fallback is
 *  always usable, a blank embed is not. */
export function webviewCanShowPdf(): boolean {
  try {
    return typeof navigator !== 'undefined' && (navigator as any).pdfViewerEnabled === true;
  } catch {
    return false;
  }
}

export const PdfViewer: React.FC<PdfViewerProps> = ({ data, name, path, onOpenExternal, forceFallback }) => {
  const native = !forceFallback && webviewCanShowPdf();
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!native) {
      setUrl(null);
      return;
    }
    const u = URL.createObjectURL(new Blob([data as unknown as BlobPart],{ type: 'application/pdf' }));
    setUrl(u);
    return () => URL.revokeObjectURL(u);
  }, [data, native]);

  const kb = useMemo(() => (data.byteLength / 1024).toFixed(data.byteLength > 1048576 ? 0 : 1), [data]);

  return (
    <div className="qu-viewer" data-testid="pdf-viewer">
      <div className="qu-viewer-bar">
        <span className="qu-viewer-name" title={path || name}>
          {name}
        </span>
        <span className="qu-viewer-zoom">{kb} KB</span>
        {onOpenExternal && (
          <button type="button" className="qu-viewer-btn" onClick={onOpenExternal} title="Open in the system PDF viewer">
            <ExternalLink size={15} />
            <span>Open in system viewer</span>
          </button>
        )}
      </div>
      <div className="qu-viewer-stage qu-viewer-stage-pdf">
        {native && url ? (
          <embed src={url} type="application/pdf" className="qu-viewer-embed" title={name} />
        ) : native ? null : (
          <div className="qu-viewer-fallback" role="status">
            <FileText size={40} />
            <strong>This window cannot show PDFs inline</strong>
            <p>
              The web view Qu Studio uses on this system has no built-in PDF viewer. The file is fine; open it in
              your PDF application instead.
            </p>
            {onOpenExternal && (
              <button type="button" className="qu-viewer-btn qu-viewer-btn-primary" onClick={onOpenExternal}>
                <ExternalLink size={15} />
                <span>Open in system viewer</span>
              </button>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
