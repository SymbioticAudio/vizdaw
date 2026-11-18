"use strict";
// VizDAW's MCP Apps bridge (plan 8c Ruling 7): JSON-RPC over postMessage
// with the host, which is always window.parent. It numbers its own
// requests, answers only requests the host sends, and ignores every
// message from any other source. Every value it draws is set with
// textContent, so nothing from the song can become markup.
const Bridge = (function () {
  const host = window.parent;
  const waiting = new Map();
  let next = 1;
  let draw = null;

  const STYLE = [
    ":root { --bg: #ffffff; --fg: #1d1d1f; --muted: #5f6368; --line: #d9d9de;",
    "  --tag-bg: #e8f0fe; --tag-fg: #1a4fa0; --button-bg: #f2f2f5; }",
    ":root[data-theme=\"dark\"] { --bg: #1e1e20; --fg: #ececf0; --muted: #a3a3ab;",
    "  --line: #3a3a3f; --tag-bg: #233a5e; --tag-fg: #b9d2ff; --button-bg: #2c2c30; }",
    "html, body { margin: 0; background: var(--bg); color: var(--fg); }",
    "body { font: 13px/1.45 system-ui, sans-serif; padding: 12px 14px; }",
    "h1 { font-size: 15px; margin: 0 0 8px; }",
    "h2 { font-size: 13px; margin: 12px 0 4px; color: var(--muted); }",
    "p { margin: 6px 0; }",
    "ul { margin: 0; padding-left: 18px; }",
    "li { margin: 2px 0; }",
    "table { border-collapse: collapse; width: 100%; }",
    "th, td { text-align: left; padding: 4px 8px 4px 0; border-bottom: 1px solid var(--line); vertical-align: top; }",
    "th { color: var(--muted); font-weight: 600; }",
    ".name, td, li, pre, .path { unicode-bidi: isolate; overflow-wrap: anywhere; }",
    ".tag { display: inline-block; margin-left: 6px; padding: 0 6px; border-radius: 8px;",
    "  background: var(--tag-bg); color: var(--tag-fg); font-size: 11px; }",
    ".state { font-weight: 600; }",
    ".muted { color: var(--muted); }",
    "pre { white-space: pre-wrap; font: inherit; margin: 0; }",
    "button { font: inherit; color: var(--fg); background: var(--button-bg);",
    "  border: 1px solid var(--line); border-radius: 6px; padding: 3px 10px; margin-top: 8px; }",
    "[hidden] { display: none !important; }",
  ].join("\n");

  function send(message) {
    host.postMessage(message, "*");
  }

  function request(method, params) {
    const id = next;
    next += 1;
    send({ jsonrpc: "2.0", id: id, method: method, params: params });
    return new Promise(function (resolve, reject) {
      waiting.set(id, { resolve: resolve, reject: reject });
    });
  }

  function notify(method, params) {
    send({ jsonrpc: "2.0", method: method, params: params });
  }

  function theme(context) {
    if (!context || typeof context !== "object") {
      return;
    }
    if (context.theme === "dark" || context.theme === "light") {
      document.documentElement.setAttribute("data-theme", context.theme);
    }
  }

  function answer(m) {
    if (m.method === "ping" || m.method === "ui/resource-teardown") {
      send({ jsonrpc: "2.0", id: m.id, result: {} });
    } else {
      send({ jsonrpc: "2.0", id: m.id, error: { code: -32601, message: "Method not found" } });
    }
  }

  window.addEventListener("message", function (event) {
    if (event.source !== host) {
      return;
    }
    const m = event.data;
    if (!m || typeof m !== "object" || m.jsonrpc !== "2.0") {
      return;
    }
    if (typeof m.method === "string") {
      const asked = typeof m.id === "string" || typeof m.id === "number";
      if (asked) {
        answer(m);
      } else if (m.method === "ui/notifications/tool-result") {
        if (draw) {
          draw(m.params && typeof m.params === "object" ? m.params : null);
        }
      } else if (m.method === "ui/notifications/host-context-changed") {
        theme(m.params);
      }
      return;
    }
    if (typeof m.id === "number" && waiting.has(m.id)) {
      const w = waiting.get(m.id);
      waiting.delete(m.id);
      if (m.error !== undefined) {
        w.reject(m.error);
      } else {
        w.resolve(m.result);
      }
    }
  });

  // One element, its text set as text.
  function el(tag, text, cls) {
    const e = document.createElement(tag);
    if (text !== undefined && text !== null) {
      e.textContent = String(text);
    }
    if (cls) {
      e.className = cls;
    }
    return e;
  }

  function str(v, fallback) {
    return typeof v === "string" ? v : fallback;
  }

  // The result's text content, as it is.
  function plainText(result) {
    const content = result && Array.isArray(result.content) ? result.content : [];
    return content
      .filter(function (c) { return c && c.type === "text" && typeof c.text === "string"; })
      .map(function (c) { return c.text; })
      .join("\n");
  }

  function clear(root) {
    while (root.firstChild) {
      root.removeChild(root.firstChild);
    }
  }

  function reportSize() {
    const r = document.documentElement.getBoundingClientRect();
    notify("ui/notifications/size-changed", {
      width: Math.ceil(r.width),
      height: Math.ceil(r.height),
    });
  }

  // Draws each tool result into `root`: `render` for a result whose
  // structuredContent is this `view`, else the text content as it is.
  function start(view, root, render) {
    const style = document.createElement("style");
    style.textContent = STYLE;
    document.head.appendChild(style);
    root.appendChild(el("p", "Waiting for VizDAW's answer.", "muted"));
    draw = function (result) {
      clear(root);
      const sc = result ? result.structuredContent : null;
      if (!result || result.isError === true || !sc || typeof sc !== "object" || sc.view !== view
        || render(sc, root, result) === false) {
        clear(root);
        const text = plainText(result);
        root.appendChild(el("pre", text === "" ? "VizDAW gave no answer to show." : text));
      }
      reportSize();
    };
    if (typeof ResizeObserver === "function") {
      new ResizeObserver(reportSize).observe(document.documentElement);
    }
    request("ui/initialize", {
      protocolVersion: "2026-01-26",
      appInfo: { name: "VizDAW", version: "1" },
      appCapabilities: {},
    }).then(function (r) {
      if (r && typeof r === "object") {
        theme(r.hostContext);
      }
      notify("ui/notifications/initialized", {});
    }, function () {
      notify("ui/notifications/initialized", {});
    });
  }

  return { start: start, request: request, el: el, str: str };
})();
