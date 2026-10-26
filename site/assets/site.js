// Copy buttons and the xterm-256 palette. Everything else on the site is static HTML.

(() => {
  const copyButtons = document.querySelectorAll("[data-copy]");

  copyButtons.forEach((button) => {
    button.addEventListener("click", async () => {
      const target = document.querySelector(button.dataset.copy);
      if (!target) return;

      const text = `${target.innerText.replace(/\n+$/, "")}\n`;
      const original = button.dataset.label || button.textContent;

      try {
        await navigator.clipboard.writeText(text);
      } catch {
        button.textContent = "Copy failed";
        window.setTimeout(() => {
          button.textContent = original;
        }, 2500);
        return;
      }

      button.dataset.label = original;
      button.dataset.copied = "true";
      button.textContent = "Copied";
      window.clearTimeout(button._copyTimer);
      button._copyTimer = window.setTimeout(() => {
        button.textContent = original;
        delete button.dataset.copied;
      }, 2500);
    });
  });

  const canvas = document.getElementById("xterm-palette");
  if (!canvas) return;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  // The 256-colour xterm palette (content, not theme tokens): 16 base
  // colours, a 6×6×6 cube, 24 greys.
  const cube = [0, 95, 135, 175, 215, 255];
  const colours = [
    "#000000", "#800000", "#008000", "#808000",
    "#000080", "#800080", "#008080", "#c0c0c0",
    "#808080", "#ff0000", "#00ff00", "#ffff00",
    "#0000ff", "#ff00ff", "#00ffff", "#ffffff",
  ];

  for (const red of cube) {
    for (const green of cube) {
      for (const blue of cube) {
        colours.push(`rgb(${red}, ${green}, ${blue})`);
      }
    }
  }

  for (let step = 0; step < 24; step += 1) {
    const value = 8 + step * 10;
    colours.push(`rgb(${value}, ${value}, ${value})`);
  }

  const cell = canvas.width / 16;
  colours.forEach((colour, index) => {
    const x = (index % 16) * cell;
    const y = Math.floor(index / 16) * cell;
    ctx.fillStyle = colour;
    ctx.fillRect(x, y, cell - 1, cell - 1);
  });
})();
