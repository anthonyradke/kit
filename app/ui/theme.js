"use strict";
// Loaded in <head> so the saved theme is on <html> before the first paint.
document.documentElement.dataset.theme = localStorage.getItem("kit.theme") || "graphite";
