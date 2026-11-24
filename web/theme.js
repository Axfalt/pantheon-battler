// Pantheon themes: a click on a footer plate dresses the page in that pantheon's colours; a
// second click on the same plate goes back to the generic theme. The choice is remembered in
// this browser only (the page works the same without it).
(function () {
  "use strict";
  const KEY = "arene-theme";
  const THEMES = ["egypte", "rome", "gaule", "nord"];
  const root = document.documentElement;
  const buttons = document.querySelectorAll(".plaques button[data-theme]");

  function apply(theme) {
    if (THEMES.includes(theme)) {
      root.dataset.theme = theme;
    } else {
      delete root.dataset.theme;
    }
    buttons.forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.theme === root.dataset.theme)));
    try {
      if (root.dataset.theme) localStorage.setItem(KEY, root.dataset.theme);
      else localStorage.removeItem(KEY);
    } catch (e) {
      // Private window or blocked storage: the theme just isn't remembered.
    }
  }

  buttons.forEach((b) =>
    b.addEventListener("click", () => apply(root.dataset.theme === b.dataset.theme ? null : b.dataset.theme)),
  );
  apply(root.dataset.theme);
})();
