// MiiOS -> Mii Hub bridge. MiiOS adds this one <script> tag to its copy of
// Mii Hub; Mii Hub's own code is not changed. It hands the Miis you saved in
// MiiOS (~/MiiOS/miis) to Mii Hub through Mii Hub's normal "Choose Mii .glb
// files..." button, once per file.
(function () {
  "use strict";
  var DONE_KEY = "miios-imported-files";
  function done() { try { return JSON.parse(localStorage.getItem(DONE_KEY) || "[]"); } catch (e) { return []; } }
  async function bring() {
    var input = document.getElementById("fileInput");
    if (!input) return false; // Mii Hub not ready yet
    var list = await (await fetch("/miis/list.json", { cache: "no-store" })).json();
    var seen = done();
    var dt = new DataTransfer();
    for (var i = 0; i < list.length; i++) {
      var name = list[i];
      if (seen.indexOf(name) >= 0) continue;
      var bytes = await (await fetch("/miis/" + encodeURIComponent(name), { cache: "no-store" })).arrayBuffer();
      dt.items.add(new File([bytes], name, { type: "model/gltf-binary" }));
      seen.push(name);
    }
    if (dt.files.length) {
      input.files = dt.files;
      input.dispatchEvent(new Event("change", { bubbles: true }));
      localStorage.setItem(DONE_KEY, JSON.stringify(seen));
    }
    return true;
  }
  // Wait until Mii Hub has finished starting, then bring the Miis in; check
  // again now and then for Miis you add while it's open.
  var tries = 0;
  function tick() {
    var splash = document.getElementById("splash");
    var ready = !splash || splash.offsetParent === null || getComputedStyle(splash).display === "none";
    if (ready) bring().catch(function () {});
    if (++tries < 400) setTimeout(tick, ready ? 15000 : 1000);
  }
  window.addEventListener("load", function () { setTimeout(tick, 1500); });
})();
