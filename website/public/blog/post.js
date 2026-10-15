// The copy buttons on a post. The home page has its own copy of this in
// main.js, beside code that needs elements a post doesn't have.
document.querySelectorAll(".copy").forEach((button) => {
  button.addEventListener("click", async () => {
    const source = document.getElementById(button.dataset.copy);
    try {
      await navigator.clipboard.writeText(source.textContent.trim());
      button.title = "Copied";
      button.classList.add("done");
    } catch {
      // No clipboard access: select the command so it can be copied by hand.
      const range = document.createRange();
      range.selectNodeContents(source);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      button.title = "Selected: press Ctrl+C";
    }
    setTimeout(() => {
      button.title = "Copy";
      button.classList.remove("done");
    }, 1800);
  });
});
