(function () {
  'use strict';

  // Two hooks: the video hero, and any page that just wants `data-scrolled`
  // on <html> — the homepage stage uses it to decide when its transparent
  // header needs a surface again.
  var hero = document.querySelector('.hero--video, [data-scroll-header]');
  if (!hero) return;

  var root = document.documentElement;
  if (hero.classList.contains('hero--video')) root.setAttribute('data-hero-video', '');

  var updateScrolled = function () {
    if (window.scrollY > 24) {
      root.setAttribute('data-scrolled', '');
    } else {
      root.removeAttribute('data-scrolled');
    }
  };
  updateScrolled();
  window.addEventListener('scroll', updateScrolled, { passive: true });

  var video = hero.querySelector('.hero-video');
  if (!video) return;

  var conn = navigator.connection;
  if (conn && conn.saveData) {
    video.removeAttribute('autoplay');
    video.preload = 'none';
    return;
  }

  // Motion is the entire content of a background loop, so a reduced-motion
  // request is a request not to have one. The poster stays and nothing is
  // fetched — this is the cheapest path, not a degraded one.
  var still = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)');
  if (still && still.matches) return;

  var play = function () {
    var p = video.play();
    if (p && p.catch) p.catch(function () {});
  };

  /**
   * Attach the sources late.
   *
   * The element ships with none, so the poster — 71KB — is the whole cost of
   * first paint and the terminal beside it is usable immediately. The real
   * asset is fetched once the page has gone quiet, and crossfades in when it
   * can actually play rather than popping in half-buffered.
   */
  if (video.dataset.srcWebm || video.dataset.srcMp4) {
    var attach = function () {
      [['srcWebm', 'video/webm'], ['srcMp4', 'video/mp4']].forEach(function (pair) {
        var url = video.dataset[pair[0]];
        if (!url) return;
        var source = document.createElement('source');
        source.src = url;
        source.type = pair[1];
        video.append(source);
      });
      video.addEventListener('canplay', function () {
        video.classList.add('is-ready');
        play();
      }, { once: true });
      video.load();
    };
    if (window.requestIdleCallback) {
      window.requestIdleCallback(attach, { timeout: 2500 });
    } else {
      setTimeout(attach, 1200);
    }
  }

  if ('IntersectionObserver' in window) {
    new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (entry.isIntersecting) {
          play();
        } else {
          video.pause();
        }
      });
    }, { threshold: 0.05 }).observe(hero);
  }

  document.addEventListener('visibilitychange', function () {
    if (document.hidden) {
      video.pause();
    } else if (hero.getBoundingClientRect().bottom > 0) {
      play();
    }
  });
})();
