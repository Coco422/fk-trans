import { For, type JSX } from "solid-js";

type MarkdownBlock =
  | { type: "heading"; level: number; text: string }
  | { type: "paragraph"; text: string }
  | { type: "ul"; items: string[] }
  | { type: "ol"; items: string[] }
  | { type: "blockquote"; text: string }
  | { type: "code"; text: string };

interface MarkdownViewProps {
  source: string;
}

function isBlockStart(line: string) {
  return (
    /^#{1,4}\s+/.test(line) ||
    /^\s*[-*]\s+/.test(line) ||
    /^\s*\d+\.\s+/.test(line) ||
    /^>\s?/.test(line) ||
    /^```/.test(line)
  );
}

function parseMarkdown(source: string): MarkdownBlock[] {
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const blocks: MarkdownBlock[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      index += 1;
      continue;
    }

    if (/^```/.test(line)) {
      index += 1;
      const code: string[] = [];
      while (index < lines.length && !/^```/.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      if (index < lines.length) index += 1;
      blocks.push({ type: "code", text: code.join("\n") });
      continue;
    }

    const heading = line.match(/^(#{1,4})\s+(.+)$/);
    if (heading) {
      blocks.push({
        type: "heading",
        level: heading[1].length,
        text: heading[2],
      });
      index += 1;
      continue;
    }

    if (/^\s*[-*]\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*[-*]\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\s*[-*]\s+/, ""));
        index += 1;
      }
      blocks.push({ type: "ul", items });
      continue;
    }

    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (index < lines.length && /^\s*\d+\.\s+/.test(lines[index])) {
        items.push(lines[index].replace(/^\s*\d+\.\s+/, ""));
        index += 1;
      }
      blocks.push({ type: "ol", items });
      continue;
    }

    if (/^>\s?/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^>\s?/.test(lines[index])) {
        quote.push(lines[index].replace(/^>\s?/, ""));
        index += 1;
      }
      blocks.push({ type: "blockquote", text: quote.join("\n") });
      continue;
    }

    const paragraph: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() &&
      !isBlockStart(lines[index])
    ) {
      paragraph.push(lines[index]);
      index += 1;
    }
    blocks.push({ type: "paragraph", text: paragraph.join("\n") });
  }

  return blocks;
}

function safeHref(href: string) {
  try {
    const url = new URL(href, "https://fk-trans.local");
    if (["http:", "https:", "mailto:"].includes(url.protocol)) {
      return href;
    }
  } catch {}
  return undefined;
}

function renderInline(text: string): JSX.Element[] {
  const pattern = /(`[^`]+`|\*\*[^*]+?\*\*|\[[^\]]+?\]\([^)]+?\))/g;
  const nodes: JSX.Element[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(text))) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }

    const token = match[0];
    if (token.startsWith("`")) {
      nodes.push(
        <code class="px-1 py-0.5 rounded bg-gray-100 dark:bg-gray-800 font-mono text-[0.92em]">
          {token.slice(1, -1)}
        </code>
      );
    } else if (token.startsWith("**")) {
      nodes.push(<strong class="font-semibold">{token.slice(2, -2)}</strong>);
    } else {
      const link = token.match(/^\[([^\]]+?)\]\(([^)]+?)\)$/);
      const href = link ? safeHref(link[2]) : undefined;
      nodes.push(
        href ? (
          <a
            class="text-blue-600 dark:text-blue-300 underline underline-offset-2"
            href={href}
            target="_blank"
            rel="noreferrer"
          >
            {link?.[1]}
          </a>
        ) : (
          token
        )
      );
    }

    lastIndex = match.index + token.length;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }

  return nodes;
}

export default function MarkdownView(props: MarkdownViewProps) {
  return (
    <div class="space-y-2 text-sm text-gray-700 dark:text-gray-200 leading-relaxed">
      <For each={parseMarkdown(props.source)}>
        {(block) => {
          if (block.type === "heading") {
            const headingClass =
              block.level <= 2
                ? "text-base font-semibold text-gray-900 dark:text-white"
                : "text-sm font-semibold text-gray-900 dark:text-white";
            return <div class={headingClass}>{renderInline(block.text)}</div>;
          }

          if (block.type === "ul") {
            return (
              <ul class="list-disc pl-5 space-y-1">
                <For each={block.items}>
                  {(item) => <li>{renderInline(item)}</li>}
                </For>
              </ul>
            );
          }

          if (block.type === "ol") {
            return (
              <ol class="list-decimal pl-5 space-y-1">
                <For each={block.items}>
                  {(item) => <li>{renderInline(item)}</li>}
                </For>
              </ol>
            );
          }

          if (block.type === "blockquote") {
            return (
              <blockquote class="border-l-2 border-gray-300 dark:border-gray-700 pl-3 text-gray-600 dark:text-gray-300 whitespace-pre-wrap">
                {renderInline(block.text)}
              </blockquote>
            );
          }

          if (block.type === "code") {
            return (
              <pre class="popup-scroll overflow-x-auto rounded-md bg-gray-100 dark:bg-gray-950 border border-gray-200 dark:border-gray-800 p-2 text-xs leading-relaxed">
                <code>{block.text}</code>
              </pre>
            );
          }

          return (
            <p class="whitespace-pre-wrap">
              {renderInline(block.text)}
            </p>
          );
        }}
      </For>
    </div>
  );
}
