// Stands in for window.__TAURI__ inside each app page; the stage (parent) owns all state.
(() => {
  const T = window.parent.TEASER;
  // NOTE: app bug workaround — base.css lacks [hidden]{display:none}, so .pill[hidden] still shows.
  document.addEventListener("DOMContentLoaded", () => { const st = document.createElement("style"); st.textContent = "[hidden]{display:none!important}"; document.head.appendChild(st); });
  window.__TAURI__ = {
    core: { invoke: (cmd, args) => Promise.resolve(T.invoke(cmd, args || {}, window)) },
    event: {
      listen: (ev, cb) => {
        T.on(ev, cb, window);
        return Promise.resolve(() => {});
      },
    },
    window: { getCurrentWindow: () => ({ minimize() {}, toggleMaximize() {}, close() {} }) },
  };
})();
