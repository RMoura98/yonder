/* Yonder website — small, dependency-free enhancements */
(function () {
  "use strict";
  document.documentElement.classList.remove("no-js");

  var RELEASES = "https://github.com/RMoura98/yonder/releases/latest";
  var mq = window.matchMedia ? window.matchMedia("(prefers-reduced-motion: reduce)") : null;
  var reduceMotion = !!(mq && mq.matches);
  function pad(n) { return (n < 10 ? "0" : "") + n; }

  /* ---------- OS detection for download buttons ---------- */
  function detectOS() {
    var uaData = navigator.userAgentData;
    var p = ((uaData && uaData.platform) || navigator.platform || "").toLowerCase();
    var ua = (navigator.userAgent || "").toLowerCase();
    if (/android|iphone|ipad|ipod/.test(ua)) return null;
    if (p.indexOf("win") > -1 || ua.indexOf("windows") > -1) return "windows";
    if (p.indexOf("mac") > -1 || ua.indexOf("mac os") > -1) return "macos";
    if (p.indexOf("linux") > -1 || ua.indexOf("linux") > -1 || ua.indexOf("x11") > -1) return "linux";
    return null;
  }
  var names = { windows: "Windows", macos: "macOS", linux: "Linux" };
  var os = detectOS();
  document.querySelectorAll("[data-download]").forEach(function (el) {
    el.setAttribute("href", RELEASES);
    var label = el.querySelector("[data-download-label]");
    if (label && os) label.textContent = "Download for " + names[os];
    var icon = el.querySelector("[data-os-icon]");
    if (icon && os) {
      icon.querySelectorAll("[data-os]").forEach(function (g) {
        g.style.display = g.getAttribute("data-os") === os ? "" : "none";
      });
    }
  });
  if (os) {
    document.querySelectorAll("[data-other-os]").forEach(function (el) {
      var others = Object.keys(names).filter(function (k) { return k !== os; });
      el.textContent = "Also available for ";
      others.forEach(function (k, i) {
        var a = document.createElement("a");
        a.href = RELEASES; a.textContent = names[k];
        el.appendChild(a);
        if (i < others.length - 1) el.appendChild(document.createTextNode(" and "));
      });
      el.appendChild(document.createTextNode(" — or build from source."));
    });
  }

  /* ---------- Sticky nav border ---------- */
  var nav = document.querySelector(".nav");
  if (nav) {
    var onScrollNav = function () { nav.classList.toggle("scrolled", window.scrollY > 8); };
    onScrollNav();
    window.addEventListener("scroll", onScrollNav, { passive: true });
  }

  /* ---------- Copy buttons ---------- */
  document.querySelectorAll("[data-copy]").forEach(function (btn) {
    btn.addEventListener("click", function () {
      var target = document.getElementById(btn.getAttribute("data-copy"));
      if (!target) return;
      var text = target.innerText.replace(/^\$\s?/gm, "").trim();
      var done = function () {
        var label = btn.querySelector("span");
        var prev = label ? label.textContent : "";
        btn.classList.add("copied");
        if (label) label.textContent = "Copied";
        setTimeout(function () { btn.classList.remove("copied"); if (label) label.textContent = prev; }, 1800);
      };
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(done, function () { fallbackCopy(text); done(); });
      } else { fallbackCopy(text); done(); }
    });
  });
  function fallbackCopy(text) {
    var ta = document.createElement("textarea");
    ta.value = text; ta.setAttribute("readonly", ""); ta.style.position = "fixed"; ta.style.opacity = "0";
    document.body.appendChild(ta); ta.select();
    try { document.execCommand("copy"); } catch (e) { /* ignore */ }
    document.body.removeChild(ta);
  }

  /* ---------- Scroll reveal ---------- */
  var reveals = document.querySelectorAll(".reveal");
  if (reduceMotion || !("IntersectionObserver" in window)) {
    reveals.forEach(function (el) { el.classList.add("in"); });
  } else {
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); }
      });
    }, { rootMargin: "0px 0px -8% 0px", threshold: 0.08 });
    reveals.forEach(function (el) { io.observe(el); });
  }

  /* ---------- Hero demo: recreation of the app flow ---------- */
  var demo = document.getElementById("demo");
  if (demo) {
    var huCount = document.getElementById("hu-count");
    var curCount = document.getElementById("cur-count");
    var prog = document.getElementById("brk-prog");
    var strips = {};
    demo.querySelectorAll(".odo .strip").forEach(function (s) {
      for (var i = 0; i <= 9; i++) { var d = document.createElement("span"); d.textContent = i; s.appendChild(d); }
      strips[s.getAttribute("data-d")] = s;
    });
    var BREAK = 20;
    var setOdo = function (secs) {
      strips.tens.style.transform = "translateY(" + (-Math.floor(secs / 10)) + "em)";
      strips.ones.style.transform = "translateY(" + (-(secs % 10)) + "em)";
      prog.style.transform = "scaleX(" + (secs / BREAK) + ")";
    };
    var setPhase = function (p) { demo.setAttribute("data-phase", p); };

    if (reduceMotion) {
      setPhase("break"); setOdo(BREAK);
    } else {
      // timeline: [delay before step, action]
      var steps = [
        [0, function () { setPhase("desktop"); setOdo(BREAK); huCount.textContent = "00:05"; }],
        [1800, function () { setPhase("toast"); }],
        [1000, function () { huCount.textContent = "00:04"; }],
        [1000, function () { huCount.textContent = "00:03"; }],
        [1000, function () { setPhase("cursor"); curCount.textContent = "3"; }],
        [900, function () { curCount.textContent = "2"; }],
        [900, function () { curCount.textContent = "1"; }],
        [900, function () { setPhase("break"); }]
      ];
      for (var s = BREAK - 1; s >= BREAK - 7; s--) {
        (function (v) { steps.push([1000, function () { setOdo(v); }]); })(s);
      }
      steps.push([1400, function () { setPhase("desktop"); }]);
      steps.push([1600, function () { /* rest on the desktop, then loop */ }]);

      var idx = 0, timer = null, visible = true;
      var tick = function () {
        steps[idx][1]();
        idx = (idx + 1) % steps.length;
        if (visible) timer = setTimeout(tick, steps[idx][0]);
        else timer = null;
      };
      if ("IntersectionObserver" in window) {
        new IntersectionObserver(function (entries) {
          visible = entries[0].isIntersecting;
          if (visible && !timer) timer = setTimeout(tick, 300);
        }).observe(demo);
      }
      tick();
    }
  }

  /* ---------- Why Yonder: sun follows scroll ---------- */
  var scene = document.getElementById("scene");
  if (scene) {
    if (!reduceMotion) {
      var ticking = false;
      var update = function () {
        ticking = false;
        var r = scene.getBoundingClientRect();
        var vh = window.innerHeight || 800;
        var p = (vh - r.top) / (vh + r.height * 0.4);
        p = Math.max(0, Math.min(1, p));
        scene.style.setProperty("--p", p.toFixed(3));
      };
      window.addEventListener("scroll", function () { if (!ticking) { ticking = true; requestAnimationFrame(update); } }, { passive: true });
      window.addEventListener("resize", update);
      update();
    } else {
      scene.style.setProperty("--p", "0.8");
    }

    /* 20-second "Try it now" */
    var btn = document.getElementById("try-btn");
    var cancel = document.getElementById("try-cancel");
    var tTitle = document.getElementById("try-title");
    var tCount = document.getElementById("try-count");
    var tSub = document.getElementById("try-sub");
    var tBar = document.getElementById("try-bar");
    var tInt = null;
    var stop = function () { clearInterval(tInt); tInt = null; scene.classList.remove("trying"); btn.disabled = false; };
    var start = function () {
      var left = 20;
      clearInterval(tInt);
      scene.classList.add("trying");
      btn.disabled = true;
      tTitle.textContent = "Look at something far away";
      tSub.textContent = "A window, a tree, the end of the hallway — at least 20 feet away. We'll wait.";
      tCount.textContent = left;
      tBar.style.transform = "scaleX(1)";
      cancel.textContent = "Stop";
      scene.scrollIntoView({ behavior: reduceMotion ? "auto" : "smooth", block: "center" });
      tInt = setInterval(function () {
        left--;
        tCount.textContent = left;
        tBar.style.transform = "scaleX(" + (left / 20) + ")";
        if (left <= 0) {
          clearInterval(tInt); tInt = null;
          tTitle.textContent = "Welcome back.";
          tCount.textContent = "✓";
          tSub.textContent = "That's the whole trick. Yonder will nudge you to do it again in 20 minutes.";
          cancel.textContent = "Close";
        }
      }, 1000);
    };
    if (btn) btn.addEventListener("click", start);
    if (cancel) cancel.addEventListener("click", stop);
    document.addEventListener("keydown", function (e) { if (e.key === "Escape" && scene.classList.contains("trying")) stop(); });
  }

  /* ---------- Docs: sidebar toggle + scrollspy ---------- */
  var side = document.querySelector(".docs-side");
  if (side) {
    var toggle = side.querySelector(".docs-toggle");
    if (toggle) {
      toggle.addEventListener("click", function () {
        var open = side.classList.toggle("open");
        toggle.setAttribute("aria-expanded", open ? "true" : "false");
      });
      side.querySelectorAll("nav a").forEach(function (a) {
        a.addEventListener("click", function () {
          side.classList.remove("open");
          toggle.setAttribute("aria-expanded", "false");
        });
      });
    }
    var links = {};
    side.querySelectorAll('nav a[href^="#"]').forEach(function (a) { links[a.getAttribute("href").slice(1)] = a; });
    if ("IntersectionObserver" in window) {
      var spy = new IntersectionObserver(function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting && links[e.target.id]) {
            Object.keys(links).forEach(function (k) { links[k].classList.remove("active"); });
            links[e.target.id].classList.add("active");
          }
        });
      }, { rootMargin: "-20% 0px -70% 0px" });
      document.querySelectorAll(".doc-section[id]").forEach(function (s) { spy.observe(s); });
    }
  }

  /* ---------- Footer year ---------- */
  document.querySelectorAll("[data-year]").forEach(function (el) { el.textContent = new Date().getFullYear(); });
})();

// Real recordings: respect reduced motion, and only play while on screen.
(function () {
  var vids = document.querySelectorAll(".reel video");
  if (!vids.length) return;
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    vids.forEach(function (v) { v.removeAttribute("autoplay"); v.pause(); v.controls = true; });
    return;
  }
  if (!("IntersectionObserver" in window)) return;
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) { e.isIntersecting ? e.target.play().catch(function () {}) : e.target.pause(); });
  }, { threshold: 0.2 });
  vids.forEach(function (v) { io.observe(v); });
})();
