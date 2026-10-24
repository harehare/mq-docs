// Client-side behaviour of the generated reference page. The URL is the router:
//   #mod-csv                 opens a module page
//   #csv::csv_parse / #abs   opens the page holding that entry and scrolls to it
//   ?q=parse                 shows the cross-module search results for "parse"
(function () {
  var $ = function (sel, root) {
    return (root || document).querySelector(sel);
  };
  var $$ = function (sel, root) {
    return Array.prototype.slice.call((root || document).querySelectorAll(sel));
  };

  var pages = $$(".module-page:not(.search-page)");
  var searchPage = $("#search-results");
  var links = $$(".sidebar-link");
  var sidebar = $("#sidebar");
  var overlay = $("#sidebarOverlay");
  var globalInput = $("#globalSearch");
  var MAX_RESULTS = 200;

  // The "All" page repeats the module pages' entries, but the HTML only lists their anchors:
  // fill it in by cloning the canonical cards (the ones carrying an id).
  var canonical = new Map();
  $$(".entry[id]").forEach(function (el) {
    canonical.set(el.id, el);
  });
  $$(".entries[data-anchors]").forEach(function (box) {
    var group = box.getAttribute("data-group");
    var fragment = document.createDocumentFragment();
    box
      .getAttribute("data-anchors")
      .split(" ")
      .forEach(function (anchor) {
        var source = canonical.get(anchor);
        if (!source) return;
        var copy = source.cloneNode(true);
        copy.removeAttribute("id");
        copy.setAttribute("data-group", group);
        fragment.appendChild(copy);
      });
    box.appendChild(fragment);
  });

  // One anchor can now map to several cards (one per page that shows it).
  var anchors = new Map();
  $$(".entry[data-anchor]").forEach(function (el) {
    var key = el.getAttribute("data-anchor");
    if (!anchors.has(key)) anchors.set(key, []);
    anchors.get(key).push(el);
    // What the on-page filter matches: name, aliases and description — not example output.
    el._text = [el.dataset.name, el.dataset.aliases, el.dataset.desc].join(" ").toLowerCase();
  });

  // One record per entry (its canonical card) for the cross-module search.
  var records = $$(".entry[id]").map(function (el) {
    var aliases = el.dataset.aliases ? el.dataset.aliases.toLowerCase().split(" ") : [];
    var desc = el.dataset.desc || "";
    return {
      anchor: el.dataset.anchor,
      anchorL: el.dataset.anchor.toLowerCase(),
      name: el.dataset.name,
      nameL: el.dataset.name.toLowerCase(),
      aliasesL: aliases,
      desc: desc,
      descL: desc.toLowerCase(),
      kind: el.dataset.kind,
      module: el.dataset.module,
      cap: el.dataset.cap,
      dep: el.dataset.dep === "1",
      sig: $(".entry-sig", el).textContent,
      href: $(".entry-anchor", el).getAttribute("href"),
    };
  });

  // Grouped pages list their categories under their sidebar link instead of in a bar above the
  // entries; the sidebar shows them only while that link is the active one.
  pages.forEach(function (page) {
    var groups = $$(".entry-group:not(.entry-group-module)", page);
    var link = links.find(function (l) {
      return l.getAttribute("data-module") === page.id;
    });
    if (!groups.length || !link) return;
    var sub = document.createElement("div");
    sub.className = "sidebar-sub";
    sub.setAttribute("aria-label", "Categories");
    groups.forEach(function (group) {
      var title = $(".group-title", group).firstChild.textContent;
      var a = document.createElement("a");
      a.className = "sidebar-sub-link";
      a.href = "#";
      a.setAttribute("data-group", title);
      a.textContent = title;
      sub.appendChild(a);
    });
    link.parentNode.insertBefore(sub, link.nextSibling);
    page._subnav = sub;
  });

  var query = "";
  var results = [];
  var selected = -1;

  function activePage() {
    return pages.find(function (p) {
      return p.classList.contains("active");
    });
  }

  function showPage(page) {
    searchPage.classList.remove("active");
    pages.forEach(function (p) {
      p.classList.toggle("active", p === page);
    });
    links.forEach(function (l) {
      l.classList.toggle("active", l.getAttribute("data-module") === page.id);
    });
    updateJumpBar();
  }

  function readHash() {
    var raw = location.hash.slice(1);
    try {
      return decodeURIComponent(raw);
    } catch (e) {
      return raw;
    }
  }

  function readQuery() {
    return new URLSearchParams(location.search).get("q") || "";
  }

  // Prefer the card on the page being viewed, then the canonical card (the one carrying the id).
  function pickEntry(key) {
    var list = anchors.get(key);
    if (!list) return null;
    var active = activePage();
    return (
      list.find(function (el) {
        return active && active.contains(el);
      }) ||
      list.find(function (el) {
        return el.id;
      }) ||
      list[0]
    );
  }

  var flashTimer;
  function flash(el) {
    clearTimeout(flashTimer);
    $$(".entry.flash").forEach(function (e) {
      e.classList.remove("flash");
    });
    // Force a reflow so the animation restarts when the same card is targeted twice.
    void el.offsetWidth;
    el.classList.add("flash");
    flashTimer = setTimeout(function () {
      el.classList.remove("flash");
    }, 1800);
  }

  // ---- Rendering the current location ----

  function render() {
    if (query.trim()) {
      showSearch();
      return;
    }

    var key = readHash();
    var page = key
      ? pages.find(function (p) {
          return p.id === key;
        })
      : null;
    var entry = !page && key ? pickEntry(key) : null;

    if (entry) {
      var owner = entry.closest(".module-page");
      if (owner !== activePage()) showPage(owner);
      if (entry.hidden) clearPageFilter(owner); // a hidden target would make the link a no-op
      entry.scrollIntoView({ block: "start" });
      flash(entry);
    } else {
      showPage(page || pages[0]);
      window.scrollTo(0, 0);
    }
  }

  // The URL changed under us (back/forward, edited hash, first load): it is the source of truth.
  function onLocationChange() {
    query = readQuery();
    globalInput.value = query;
    render();
  }

  // Navigate to `href` (a "#fragment"), leaving any search behind.
  function go(href) {
    var url = location.pathname + href;
    if (url !== location.pathname + location.search + location.hash) {
      try {
        history.pushState(null, "", url);
      } catch (e) {
        location.hash = href;
      }
    }
    query = "";
    globalInput.value = "";
    render();
  }

  function setQuery(value) {
    query = value;
    var url = location.pathname + (value.trim() ? "?q=" + encodeURIComponent(value) : "") + location.hash;
    try {
      history.replaceState(null, "", url);
    } catch (e) {
      // file:// pages may refuse; the search still works, it just isn't kept in the URL.
    }
    render();
  }

  // ---- Cross-module search ----

  function terms(q) {
    return q.toLowerCase().split(/\s+/).filter(Boolean);
  }

  // Lower is better: exact name, name prefix, name contains, then description.
  function score(rec, ts) {
    var total = 0;
    for (var i = 0; i < ts.length; i++) {
      var t = ts[i];
      var s;
      var aliases = rec.aliasesL;
      var sep = t.indexOf("::");
      if (sep > 0) {
        // `module::name` typed in full or in part: the module prefix must match, then rank by name.
        var scope = rec.anchorL.indexOf("::") > 0 ? rec.anchorL.split("::")[0] : "";
        var part = t.slice(sep + 2);
        if (!scope.startsWith(t.slice(0, sep)) || (part && rec.nameL.indexOf(part) < 0)) return -1;
        total += !part || rec.nameL === part ? 0 : rec.nameL.startsWith(part) ? 1 : 2;
        continue;
      }
      if (rec.nameL === t || rec.anchorL === t || aliases.indexOf(t) >= 0) {
        s = 0;
      } else if (
        rec.nameL.startsWith(t) ||
        rec.anchorL.startsWith(t) ||
        aliases.some(function (a) {
          return a.startsWith(t);
        })
      ) {
        s = 1;
      } else if (rec.nameL.indexOf("_" + t) >= 0) {
        s = 1.5;
      } else if (
        rec.nameL.indexOf(t) >= 0 ||
        rec.anchorL.indexOf(t) >= 0 ||
        aliases.some(function (a) {
          return a.indexOf(t) >= 0;
        })
      ) {
        s = 2;
      } else if (rec.descL.indexOf(t) >= 0) {
        s = 3;
      } else {
        return -1;
      }
      total += s;
    }
    return rec.dep ? total + 0.5 : total;
  }

  function escapeHtml(s) {
    return s
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  // Escape `text`, wrapping every occurrence of a search term in <mark>.
  function highlight(text, ts) {
    var lower = text.toLowerCase();
    if (lower.length !== text.length) return escapeHtml(text);

    var ranges = [];
    ts.forEach(function (t) {
      for (var i = lower.indexOf(t); i >= 0; i = lower.indexOf(t, i + t.length)) {
        ranges.push([i, i + t.length]);
      }
    });
    ranges.sort(function (a, b) {
      return a[0] - b[0];
    });

    var out = "";
    var pos = 0;
    ranges.forEach(function (r) {
      if (r[1] <= pos) return;
      var from = Math.max(r[0], pos);
      out += escapeHtml(text.slice(pos, from)) + "<mark>" + escapeHtml(text.slice(from, r[1])) + "</mark>";
      pos = r[1];
    });
    return out + escapeHtml(text.slice(pos));
  }

  // `module::name` highlights as its two halves.
  function highlightTerms(ts) {
    return ts
      .reduce(function (all, t) {
        return all.concat(t.split("::"));
      }, [])
      .filter(Boolean);
  }

  function snippet(rec, marks) {
    if (!rec.desc) return "";
    var first = -1;
    marks.forEach(function (t) {
      var i = rec.descL.indexOf(t);
      if (i >= 0 && (first < 0 || i < first)) first = i;
    });
    var start = Math.max(0, first - 60);
    var text = rec.desc.slice(start, start + 180);
    return (start > 0 ? "…" : "") + highlight(text, marks) + (start + 180 < rec.desc.length ? "…" : "");
  }

  function searchFilters() {
    var f = { kind: "", cap: false, dep: false };
    $$(".chip[aria-pressed='true']", searchPage).forEach(function (chip) {
      var name = chip.dataset.filter;
      if (name.indexOf("kind:") === 0) f.kind = name.slice(5);
      else f[name] = true;
    });
    return f;
  }

  function showSearch() {
    var wasActive = searchPage.classList.contains("active");
    pages.forEach(function (p) {
      p.classList.remove("active");
    });
    links.forEach(function (l) {
      l.classList.remove("active");
    });
    searchPage.classList.add("active");

    var ts = terms(query);
    var marks = highlightTerms(ts);
    var f = searchFilters();
    results = [];
    records.forEach(function (rec) {
      if ((f.kind && rec.kind !== f.kind) || (f.cap && !rec.cap) || (f.dep && !rec.dep)) return;
      var s = score(rec, ts);
      if (s >= 0) results.push({ rec: rec, score: s });
    });
    results.sort(function (a, b) {
      return a.score - b.score || a.rec.nameL.length - b.rec.nameL.length || (a.rec.nameL < b.rec.nameL ? -1 : 1);
    });

    $(".search-term", searchPage).textContent = query.trim();
    $(".count-num", searchPage).textContent = results.length;
    $(".no-results", searchPage).hidden = results.length > 0;
    $(".no-results", searchPage).textContent = "No results for “" + query.trim() + "”";

    $(".results", searchPage).innerHTML = results
      .slice(0, MAX_RESULTS)
      .map(function (r) {
        var rec = r.rec;
        var badges = "";
        if (rec.kind === "selector") badges += '<span class="badge badge-kind-selector">selector</span>';
        if (rec.dep) badges += '<span class="badge badge-deprecated">Deprecated</span>';
        if (rec.cap) badges += '<span class="badge badge-capability">requires ' + escapeHtml(rec.cap) + "</span>";
        return (
          '<a class="result" role="option" href="' + escapeHtml(rec.href) + '">' +
          '<span class="result-title"><span class="result-name">' + highlight(rec.anchor, marks) + "</span>" + badges + "</span>" +
          '<span class="result-sig">' + escapeHtml(rec.sig) + "</span>" +
          '<span class="result-desc">' + snippet(rec, marks) + "</span></a>"
        );
      })
      .join("");

    selected = -1;
    if (!wasActive) window.scrollTo(0, 0);
  }

  function moveSelection(delta) {
    var rows = $$(".result", searchPage);
    if (!rows.length) return;
    if (selected >= 0) rows[selected].classList.remove("selected");
    selected = (selected + delta + rows.length) % rows.length;
    rows[selected].classList.add("selected");
    rows[selected].scrollIntoView({ block: "nearest" });
  }

  function openSelected() {
    var rows = $$(".result", searchPage);
    var row = rows[selected >= 0 ? selected : 0];
    if (row) {
      go(row.getAttribute("href"));
      closeSidebar();
    }
  }

  globalInput.addEventListener("input", function () {
    setQuery(globalInput.value);
  });

  globalInput.addEventListener("keydown", function (e) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveSelection(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      openSelected();
    } else if (e.key === "Escape") {
      globalInput.value = "";
      setQuery("");
      globalInput.blur();
    }
  });

  document.addEventListener("keydown", function (e) {
    var t = e.target;
    var editing = t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
    var slash = e.key === "/" && !e.ctrlKey && !e.metaKey && !e.altKey && !editing;
    var ctrlK = (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k";
    if (!slash && !ctrlK) return;

    e.preventDefault();
    if (window.matchMedia("(max-width: 768px)").matches) {
      sidebar.classList.add("open");
      overlay.classList.add("open");
    }
    globalInput.focus();
    globalInput.select();
  });

  // ---- On-page filter and chips ----

  function clearPageFilter(page) {
    var input = $(".search-input", page);
    if (input) input.value = "";
    $$(".chip", page).forEach(function (chip) {
      chip.setAttribute("aria-pressed", "false");
    });
    applyPageFilter(page);
  }

  function applyPageFilter(page) {
    var input = $(".search-input", page);
    var raw = input.value.trim();
    var ts = terms(raw);
    var f = { cap: false, dep: false };
    $$(".chip[aria-pressed='true']", page).forEach(function (chip) {
      f[chip.dataset.filter] = true;
    });

    var visible = 0;
    $$(".entry", page).forEach(function (el) {
      var show =
        ts.every(function (t) {
          return el._text.indexOf(t) >= 0;
        }) &&
        (!f.cap || el.dataset.cap) &&
        (!f.dep || el.dataset.dep === "1");
      el.hidden = !show;
      if (show) visible++;
    });
    $$(".entry-group", page).forEach(function (group) {
      group.hidden = !$(".entry:not([hidden])", group);
    });

    $(".count-num", page).textContent = visible;
    var none = $(".no-results", page);
    none.hidden = visible > 0;
    none.textContent = raw ? "No results for “" + raw + "”" : "No results";
    updateJumpBar();
  }

  $$(".search-input").forEach(function (input) {
    input.addEventListener("input", function () {
      applyPageFilter(input.closest(".module-page"));
    });
  });

  // ---- Jump bar: A–Z letters, or categories in the sidebar on grouped pages ----

  function firstVisible(page, group) {
    return $('.entry[data-group="' + CSS.escape(group) + '"]:not([hidden])', page);
  }

  // The element a jump link scrolls to: the category heading, or the first card of the letter.
  function jumpTarget(first) {
    var group = first.closest(".entry-group");
    return group ? $(".group-title", group) : first;
  }

  // Dim groups with no visible entry, highlight the one being read.
  function updateJumpBar() {
    var page = activePage();
    var nav = page && ($(".az-nav", page) || page._subnav);
    if (!nav) return;

    // The toolbar is sticky and its height changes when the bar wraps; scroll-margin follows it.
    var toolbar = $(".page-toolbar", page);
    document.documentElement.style.setProperty("--toolbar-h", toolbar.offsetHeight + "px");
    var limit = toolbar.getBoundingClientRect().bottom + 24;
    var current = null;
    $$("a", nav).forEach(function (a) {
      var first = firstVisible(page, a.getAttribute("data-group"));
      a.classList.toggle("disabled", !first);
      if (first && jumpTarget(first).getBoundingClientRect().top <= limit) current = a;
    });
    $$("a", nav).forEach(function (a) {
      a.classList.toggle("current", a === current);
    });
  }

  var jumpQueued = false;
  function queueJumpBar() {
    if (jumpQueued) return;
    jumpQueued = true;
    requestAnimationFrame(function () {
      jumpQueued = false;
      updateJumpBar();
    });
  }

  // ---- Misc ----

  function closeSidebar() {
    sidebar.classList.remove("open");
    overlay.classList.remove("open");
  }

  document.addEventListener("click", function (e) {
    var chip = e.target.closest(".chip");
    if (chip) {
      var page = chip.closest(".module-page");
      var pressed = chip.getAttribute("aria-pressed") !== "true";
      if (pressed && chip.dataset.filter.indexOf("kind:") === 0) {
        // Function and selector are alternatives, not a combination.
        $$(".chip[data-filter^='kind:']", page).forEach(function (c) {
          c.setAttribute("aria-pressed", "false");
        });
      }
      chip.setAttribute("aria-pressed", String(pressed));
      if (page === searchPage) showSearch();
      else applyPageFilter(page);
      return;
    }

    var link = e.target.closest("a");
    if (!link) return;

    if (link.classList.contains("sidebar-link") || link.classList.contains("xref") || link.classList.contains("result")) {
      e.preventDefault();
      go(link.getAttribute("href"));
      closeSidebar();
    } else if (link.classList.contains("entry-anchor")) {
      e.preventDefault();
      go(link.getAttribute("href"));
    } else if (link.closest(".az-nav, .sidebar-sub")) {
      e.preventDefault();
      var first = firstVisible(activePage(), link.getAttribute("data-group"));
      if (first) jumpTarget(first).scrollIntoView({ block: "start" });
      if (link.closest(".sidebar-sub")) closeSidebar();
    }
  });

  // Mobile sidebar toggle
  $("#sidebarToggle").addEventListener("click", function () {
    sidebar.classList.toggle("open");
    overlay.classList.toggle("open");
  });
  overlay.addEventListener("click", closeSidebar);

  window.addEventListener("popstate", onLocationChange);
  window.addEventListener("hashchange", onLocationChange);
  window.addEventListener("scroll", queueJumpBar, { passive: true });
  window.addEventListener("resize", queueJumpBar);

  onLocationChange();
})();
