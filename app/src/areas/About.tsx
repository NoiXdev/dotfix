import { useEffect, useState } from "react";

import { checkUpdate, openLink, readAbout } from "../api";
import type { About as AboutData, UpdateCheck } from "../types";

/**
 * Version and where to read more.
 *
 * Its own area rather than a footnote under Settings: this is the first
 * thing someone looks for when they want to report something or find out
 * what a screen means, and a place they have to know about in advance is a
 * place they will not find.
 *
 * Links go through `open_link` rather than an `<a href>`: the window is a
 * webview with no browser chrome, so a plain link navigates the app itself
 * to the page and leaves no way back.
 */
export default function About() {
  const [data, setData] = useState<AboutData | null>(null);
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    readAbout().then(setData, (err: Error) => setError(err.message));
    // Separate from the version: the panel must paint before the network
    // answers, and a failure here is not an error banner.
    checkUpdate().then(setUpdate, () =>
      setUpdate({
        state: "unknown",
        version: null,
        url: null,
        reason: null,
      }),
    );
  }, []);

  return (
    <div className="flex flex-col gap-4 px-1 pb-2">
      <div>
        <h2 className="text-[15px] font-semibold text-ink">dotfix</h2>
        <p className="text-sm text-ink-muted">
          Keep macOS terminal setups in sync across machines
        </p>
        <p className="mt-1 text-xs text-ink-muted">
          {data ? `Version ${data.version}` : "…"}
        </p>
        {update === null ? (
          <p className="mt-1 text-xs text-ink-muted">Checking for updates…</p>
        ) : update.state === "newer" && update.version ? (
          <p className="mt-1 text-xs">
            <button
              type="button"
              className="text-decision underline underline-offset-2"
              onClick={() => {
                if (update.url) {
                  openLink(update.url).catch((err: Error) =>
                    setError(err.message),
                  );
                }
              }}
            >
              {`Version ${update.version} is available`}
            </button>
          </p>
        ) : update.state === "current" ? (
          <p className="mt-1 text-xs text-ink-muted">
            You are on the newest release.
          </p>
        ) : (
          <p className="mt-1 text-xs text-ink-muted">
            Could not check for a newer release.
          </p>
        )}
      </div>

      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}

      <ul className="flex flex-col gap-2">
        {(data?.links ?? []).map((link) => (
          <li key={link.url}>
            <button
              type="button"
              className="text-sm text-ink underline underline-offset-2"
              onClick={() => {
                openLink(link.url).catch((err: Error) =>
                  setError(err.message),
                );
              }}
            >
              {link.label}
            </button>
            <span className="ml-2 text-xs text-ink-muted">{link.url}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
