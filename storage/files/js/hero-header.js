(function () {
  'use strict';

  var hero = document.querySelector('.hero--video');
  if (!hero) return;

  var root = document.documentElement;
  root.setAttribute('data-hero-video', '');

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

  var play = function () {
    var p = video.play();
    if (p && p.catch) p.catch(function () {});
  };

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
