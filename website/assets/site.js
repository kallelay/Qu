// Shared header behaviour: theme toggle and current-page marking.
// Small enough to inline, kept separate so every page gets the same one.
(function () {
  var KEY = "qu-theme";
  var root = document.documentElement;

  try {
    var saved = localStorage.getItem(KEY);
    if (saved) root.setAttribute("data-theme", saved);
  } catch (e) { /* private window, or storage blocked: system theme stands */ }

  document.addEventListener("click", function (ev) {
    var btn = ev.target.closest("[data-theme-toggle]");
    if (!btn) return;
    var now = root.getAttribute("data-theme");
    var next = now === "dark" ? "light" : now === "light" ? "dark"
      : (matchMedia("(prefers-color-scheme: dark)").matches ? "light" : "dark");
    root.setAttribute("data-theme", next);
    try { localStorage.setItem(KEY, next); } catch (e) {}
  });

  // Mark the nav link for the page being viewed.
  var here = location.pathname.split("/").pop() || "index.html";
  document.querySelectorAll(".topnav a").forEach(function (a) {
    if (a.getAttribute("href") === here) a.setAttribute("aria-current", "page");
  });

  // Reveal sections and cards as they scroll into view. Entirely additive:
  // a page with JS off, or a browser without IntersectionObserver, or a
  // reader who asked for reduced motion, just sees everything already
  // there, since nothing is ever hidden without this also running.
  try {
    var reduceMotion = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if ("IntersectionObserver" in window && !reduceMotion) {
      var targets = document.querySelectorAll("section, .demo, .cards > div");
      var io = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) return;
          entry.target.classList.add("in-view");
          io.unobserve(entry.target);
        });
            // threshold 0, NOT a fraction. `threshold: 0.12` asks for 12% of the
      // TARGET to be visible, and the most of a target you can ever see is
      // one viewport's worth -- so anything taller than viewport/0.12, about
      // 8 screens, can never reach it and stays at `opacity: 0` forever.
      //
      // Measured on refs.html at 1280x900: 6 of its 11 sections are past
      // that, the first one included (13,323px, peaks at 6.8%). The page
      // therefore opened blank, which is how it was reported.
      //
      // rootMargin is what actually delays the reveal until an element is
      // properly on screen; the threshold was never doing that job.
    }, { threshold: 0, rootMargin: "0px 0px -40px 0px" });
      // Cards in the same row settle in as a small cascade rather than all
      // at once; the count resets every 6 so a long grid (63 demo tiles)
      // doesn't queue up one long wave by the time it scrolls into view.
      targets.forEach(function (el, i) {
        el.classList.add("reveal");
        el.style.setProperty("--reveal-delay", (i % 6) * 55 + "ms");
        io.observe(el);
      });
      // Safety net: nothing stays invisible because an observer did not
      // fire. The threshold bug above hid six of refs.html's eleven
      // sections outright, and while diagnosing it a SECOND way to get
      // the same outcome turned up -- IntersectionObserver does not fire
      // at all in a document the browser considers hidden, so the page
      // was blank there too, at any threshold.
      //
      // Both are the same shape: this is a documentation site, and the
      // text is readable only for as long as an observer behaves. That is
      // the wrong way round. An animation is worth having; it is not
      // worth the page being unreadable when it does not run.
      //
      // So everything reveals within 2.5s regardless. Anything scrolled
      // to before then still animates normally -- this only ever ends a
      // wait, it never starts one.
      setTimeout(function () {
        targets.forEach(function (el) { el.classList.add("in-view"); });
      }, 2500);
    }
  } catch (e) { /* no observer support: leave everything visible */ }
})();
