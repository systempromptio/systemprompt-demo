export const ENDPOINT = '/track/engagement';
export const MIN_TIME_MS = 5000;
export const RAGE_CLICK_THRESHOLD = 3;
export const RAGE_CLICK_WINDOW_MS = 500;
export const SCROLL_MILESTONES = [25, 50, 75, 90, 100];

export function createState() {
  return {
    pageLoadTime: Date.now(),
    firstInteractionTime: null,
    firstScrollTime: null,
    maxScrollDepth: 0,
    scrollPositions: [],
    scrollDirectionChanges: 0,
    lastScrollDirection: null,
    clickCount: 0,
    clickTimestamps: [],
    hasRageClick: false,
    hasDeadClick: false,
    mouseDistance: 0,
    lastMousePosition: null,
    keyboardEvents: 0,
    copyEvents: 0,
    focusTime: 0,
    blurCount: 0,
    tabSwitches: 0,
    visibleTime: 0,
    hiddenTime: 0,
    lastVisibilityChange: Date.now(),
    isVisible: !document.hidden,
    dataSent: false,
    pageViewSent: false,
    scrollMilestonesSent: {},
    linkClicks: []
  };
}

export function getScrollDepth() {
  const windowHeight = window.innerHeight;
  const documentHeight = Math.max(
    document.body.scrollHeight,
    document.body.offsetHeight,
    document.documentElement.scrollHeight,
    document.documentElement.offsetHeight
  );
  const scrollTop = window.scrollY || document.documentElement.scrollTop;

  if (documentHeight <= windowHeight) {
    return 100;
  }

  return Math.min(100, Math.round((scrollTop + windowHeight) / documentHeight * 100));
}

export function calculateScrollVelocity(state) {
  if (state.scrollPositions.length < 2) {
    return null;
  }

  const recent = state.scrollPositions.slice(-10);
  let totalVelocity = 0;

  for (let i = 1; i < recent.length; i++) {
    const timeDiff = recent[i].time - recent[i - 1].time;
    const posDiff = Math.abs(recent[i].position - recent[i - 1].position);
    if (timeDiff > 0) {
      totalVelocity += posDiff / timeDiff;
    }
  }

  return Math.round(totalVelocity / (recent.length - 1) * 1000);
}

export function detectRageClick(state, timestamp) {
  state.clickTimestamps.push(timestamp);

  const recentClicks = state.clickTimestamps.filter((t) => timestamp - t < RAGE_CLICK_WINDOW_MS);

  state.clickTimestamps = recentClicks;

  if (recentClicks.length >= RAGE_CLICK_THRESHOLD) {
    state.hasRageClick = true;
  }
}

export function detectReadingPattern(state) {
  const timeOnPage = Date.now() - state.pageLoadTime;
  const scrollDepth = state.maxScrollDepth;

  if (timeOnPage < 10000 && scrollDepth < 25) {
    return 'bounce';
  }

  if (scrollDepth > 75 && timeOnPage > 30000) {
    return 'engaged';
  }

  if (scrollDepth > 50 && timeOnPage > 15000) {
    return 'reader';
  }

  if (scrollDepth > 30 && timeOnPage < 20000) {
    return 'scanner';
  }

  return 'skimmer';
}

export function buildEngagementData(state) {
  const now = Date.now();
  const timeOnPage = now - state.pageLoadTime;
  const visibleTime = Math.round(state.visibleTime + (state.isVisible ? now - state.lastVisibilityChange : 0));
  const hiddenTime = Math.round(state.hiddenTime + (!state.isVisible ? now - state.lastVisibilityChange : 0));

  return {
    page_url: window.location.pathname,
    time_on_page_ms: Math.round(timeOnPage),
    max_scroll_depth: state.maxScrollDepth,
    click_count: state.clickCount,
    focus_time_ms: visibleTime,
    blur_count: state.tabSwitches || 0,
    tab_switches: state.tabSwitches || 0,
    visible_time_ms: visibleTime,
    hidden_time_ms: hiddenTime,
    time_to_first_interaction_ms: state.firstInteractionTime
      ? Math.round(state.firstInteractionTime - state.pageLoadTime)
      : null,
    time_to_first_scroll_ms: state.firstScrollTime
      ? Math.round(state.firstScrollTime - state.pageLoadTime)
      : null,
    scroll_velocity_avg: calculateScrollVelocity(state),
    scroll_direction_changes: state.scrollDirectionChanges,
    mouse_move_distance_px: Math.round(state.mouseDistance),
    keyboard_events: state.keyboardEvents,
    copy_events: state.copyEvents,
    is_rage_click: state.hasRageClick,
    is_dead_click: state.hasDeadClick,
    reading_pattern: detectReadingPattern(state)
  };
}

export function sendEvent(eventType, eventData) {
  const payload = {
    page_url: window.location.pathname,
    data: {
      ...eventData,
      event_type: eventType
    }
  };

  const jsonPayload = JSON.stringify(payload);

  if (navigator.sendBeacon) {
    const blob = new Blob([jsonPayload], { type: 'application/json' });
    navigator.sendBeacon(ENDPOINT, blob);
  } else {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', ENDPOINT, true);
    xhr.setRequestHeader('Content-Type', 'application/json');
    xhr.withCredentials = true;
    xhr.send(jsonPayload);
  }
}
