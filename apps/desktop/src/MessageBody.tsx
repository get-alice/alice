import type { MouseEvent, ReactNode } from 'react';

/** Plain-text email with its web addresses as links. `onLinkMenu` answers a right-click on one. */
export default function MessageBody({ body, onOpen, onLinkMenu }: { body: string; onOpen: (url: string) => void; onLinkMenu: (event: MouseEvent, url: string) => void }) {
  return <div className="message-body">{body.split(/\n\s*\n/).map((paragraph, index) => {
    const parts: ReactNode[] = [];
    let end = 0;
    // Control characters are never part of a URL, so a match stops at the first one.
    // eslint-disable-next-line no-control-regex
    for (const match of paragraph.matchAll(/\bhttps?:\/\/[^\s<>"\u0000-\u001f]+/gi)) {
      let url = match[0].replace(/[.,!?;:]+$/, '');
      // Keep balanced URL parentheses, but leave surrounding sentence punctuation as text.
      for (const [opening, closing] of [['(', ')'], ['[', ']'], ['{', '}']]) {
        while (url.endsWith(closing) && url.split(closing).length > url.split(opening).length) url = url.slice(0, -1);
      }
      try {
        const parsed = new URL(url);
        if (!parsed.hostname || parsed.username || parsed.password) continue;
      } catch { continue; }
      parts.push(paragraph.slice(end, match.index));
      parts.push(<a key={match.index} href={url} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer" title={url} onClick={event => { event.preventDefault(); onOpen(url); }} onContextMenu={event => onLinkMenu(event, url)}>{url}</a>);
      end = match.index + url.length;
    }
    parts.push(paragraph.slice(end));
    // Paragraphs are cut from a body that never changes while shown, so position is their identity.
    // eslint-disable-next-line @eslint-react/no-array-index-key
    return <p key={index}>{parts}</p>;
  })}</div>;
}
