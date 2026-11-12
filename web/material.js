import { liquidGlass } from "/glasswork/lens.js";

const reducedTransparency = matchMedia(
  "(prefers-reduced-transparency: reduce)",
);
const lenses = new Map();

function syncLenses() {
  for (const [element, lens] of lenses) {
    if (element.isConnected) continue;
    lens.destroy();
    lenses.delete(element);
  }
  for (const element of document.querySelectorAll("[data-lens]")) {
    if (lenses.has(element)) continue;
    lenses.set(
      element,
      liquidGlass(element, {
        rim: 13,
        edge: 1.85,
        dispersion: 0.38,
        enabled: !reducedTransparency.matches,
      }),
    );
  }
}

syncLenses();
new MutationObserver(syncLenses).observe(document.querySelector("#main"), {
  childList: true,
});

reducedTransparency.addEventListener("change", ({ matches }) => {
  for (const lens of lenses.values()) lens.update({ enabled: !matches });
});
