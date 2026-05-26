import { openUrl } from "@tauri-apps/plugin-opener";
import type { Components } from "react-markdown";
import { memo, useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

import { cn } from "@/lib/utils";

function htmlImgToMarkdown(tag: string): string {
  const src = tag.match(/\bsrc=["']([^"']+)["']/i)?.[1];
  if (!src) return "";
  const alt = tag.match(/\balt=["']([^"']*)["']/i)?.[1] ?? "";
  return `![${alt}](${src})`;
}

function stripHtmlTags(text: string): string {
  return text.replace(/<[^>]+>/g, "").trim();
}

function sanitizeProjectBody(raw: string): string {
  if (!raw) return "";
  try {
    return raw
      .replace(/<iframe[\s\S]*?<\/iframe>/gi, "")
      .replace(/<!--[\s\S]*?-->/g, "")
      .replace(/<center\b[^>]*>([\s\S]*?)<\/center>/gi, "\n\n$1\n\n")
      .replace(/<img[\s\S]*?(?:\/>|<\/img>)/gi, (tag) => htmlImgToMarkdown(tag))
      .replace(
        /<a\b[^>]*href=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi,
        (_, href, label) => {
          const text = stripHtmlTags(label) || href;
          return `[${text}](${href})`;
        }
      )
      .replace(/<h1\b[^>]*>([\s\S]*?)<\/h1>/gi, (_, t) => `\n\n## ${stripHtmlTags(t)}\n\n`)
      .replace(/<h2\b[^>]*>([\s\S]*?)<\/h2>/gi, (_, t) => `\n\n### ${stripHtmlTags(t)}\n\n`)
      .replace(/<h3\b[^>]*>([\s\S]*?)<\/h3>/gi, (_, t) => `\n\n### ${stripHtmlTags(t)}\n\n`)
      .replace(/<h4\b[^>]*>([\s\S]*?)<\/h4>/gi, (_, t) => `\n\n#### ${stripHtmlTags(t)}\n\n`)
      .replace(/<p\b[^>]*>([\s\S]*?)<\/p>/gi, (_, inner) => {
        const text = stripHtmlTags(inner);
        return text ? `\n\n${text}\n\n` : "\n\n";
      })
      .replace(/<br\s*\/?>/gi, "\n\n")
      .replace(/<hr\s*\/?>/gi, "\n\n---\n\n")
      .replace(/<[^>]+>/g, "")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  } catch {
    return raw.slice(0, 8000);
  }
}

const markdownComponents: Components = {
  a: ({ href, children }) => (
    <button
      type="button"
      className="text-[var(--color-primary)] underline underline-offset-2 hover:opacity-80 text-left"
      onClick={() => {
        if (href) void openUrl(href);
      }}
    >
      {children}
    </button>
  ),
  img: ({ src, alt }) =>
    src ? (
      <img
        src={src}
        alt={alt ?? ""}
        loading="lazy"
        className="my-3 max-w-full rounded-md border border-[var(--color-border)]"
      />
    ) : null,
  h2: ({ children }) => (
    <h2 className="mt-4 mb-2 text-sm font-semibold text-[var(--color-foreground)]">
      {children}
    </h2>
  ),
  h3: ({ children }) => (
    <h3 className="mt-4 mb-2 text-sm font-semibold text-[var(--color-foreground)]">
      {children}
    </h3>
  ),
  h4: ({ children }) => (
    <h4 className="mt-3 mb-1 text-sm font-medium text-[var(--color-foreground)]">
      {children}
    </h4>
  ),
  p: ({ children }) => (
    <p className="my-2 text-sm leading-relaxed text-[var(--color-muted-foreground)]">
      {children}
    </p>
  ),
  hr: () => <hr className="my-4 border-[var(--color-border)]" />,
  ul: ({ children }) => (
    <ul className="my-2 list-disc space-y-1 pl-5 text-sm text-[var(--color-muted-foreground)]">
      {children}
    </ul>
  ),
  ol: ({ children }) => (
    <ol className="my-2 list-decimal space-y-1 pl-5 text-sm text-[var(--color-muted-foreground)]">
      {children}
    </ol>
  ),
  li: ({ children }) => <li className="leading-relaxed">{children}</li>,
  strong: ({ children }) => (
    <strong className="font-semibold text-[var(--color-foreground)]">{children}</strong>
  ),
};

interface MarketProjectBodyProps {
  body: string;
  className?: string;
}

export const MarketProjectBody = memo(function MarketProjectBody({
  body,
  className,
}: MarketProjectBodyProps) {
  const content = useMemo(() => sanitizeProjectBody(body), [body]);
  if (!content) return null;

  return (
    <div
      className={cn(
        "mt-3 max-h-64 overflow-y-auto rounded border border-[var(--color-border)] p-3",
        className
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {content}
      </ReactMarkdown>
    </div>
  );
});
