# Stats page revamp 
## Background
The current revamp page lack of these information:
- User's daily active time
- The streak stats only included for user chat, while the review not counted as streak activity.
- The UI is not user-friendly and lack of interactive elements.
- The stats can't be control by weekly, monthly or yearly.
## Solution

### Overview
Redesign the Telemetry page into a comprehensive **User Stats** dashboard with three pillars: **Activity** (time + sessions), **Consistency** (streak + daily goals), and **Progress** (mastery + proficiency). Introduce time-range filtering (weekly/monthly/yearly) across all metrics, unify chat and review into a single activity stream for streak calculation, and add daily active time tracking.

### Key Design Decisions

1. **Unified Activity Model** — Replace the chat-only `telemetry_days` table with a generic `user_activity` table that records both chat sessions and review sessions as "activities." Streak is computed from any activity type, not just chat.

2. **Active Time Tracking** — Add a Rust backend timer that periodically (every 30s) records the user's active time when the app is in the foreground. Persist to a `daily_active_time` table keyed by date. This avoids complex foreground/background detection while being accurate enough for daily summaries.

3. **Time-Range Filter** — Add a segmented control (Weekly | Monthly | Yearly) at the top of the stats page. All charts and metric cards filter their data to the selected range. Default to "Weekly."

4. **Interactive Charts** — Replace static charts with interactive Recharts components that support tooltips, click-to-drill-down, and responsive sizing. The mastery growth chart becomes a proper line chart with hover details.

5. **Redesigned Layout** — Move from a 2x2 card grid to a more intentional layout:
   - **Hero section**: Streak + daily active time (most motivating metrics)
   - **Activity section**: Session heatmap / bar chart for the selected period
   - **Progress section**: Mastery growth + proficiency trend
   - **Review section**: SRS card state distribution + due counts

### Backend Changes (Rust/SQLite)
- New table: `user_activity (date TEXT, activity_type TEXT, count INTEGER)` — replaces `telemetry_days`
- New table: `daily_active_time (date TEXT, total_seconds INTEGER)`
- New command: `db_record_active_time` — called every 30s from frontend heartbeat
- New command: `db_get_user_activity(date_from, date_to)` — returns activity rows in range
- New command: `db_get_active_time(date_from, date_to)` — returns active time rows in range
- Modify: `db_increment_session_today` → `db_record_activity(activity_type)` — called from both Chat and Review
- Migrate existing `telemetry_days` data into `user_activity` on first launch

### Frontend Changes (React/TypeScript)
- Rewrite `src/pages/Telemetry.tsx` → `src/pages/UserStats.tsx` with new layout
- Add `src/components/stats/` directory with modular chart components:
  - `StreakCard.tsx` — large streak display with calendar heatmap
  - `ActiveTimeCard.tsx` — daily active time with trend indicator
  - `ActivityChart.tsx` — bar/heatmap chart for session distribution
  - `MasteryChart.tsx` — line chart of mastered words over time
  - `ProficiencyChart.tsx` — line chart of chat proficiency scores
  - `ReviewBreakdown.tsx` — SRS card state distribution (pie or stacked bar)
- Add `TimeRangeFilter` segmented control component
- Update `src/lib/stats.ts` — `computeStreak()` reads from unified activity; add helper functions for time-range slicing
- Update `src/pages/Chat.tsx` — call `db_record_activity('chat')` instead of `db_increment_session_today`
- Update `src/pages/Review.tsx` — call `db_record_activity('review')` after each review session completion
- Add `src/hooks/useActiveTimeHeartbeat.ts` — 30s interval hook that calls `db_record_active_time`
- Update `src/pages/Home.tsx` — update mini streak display to use new activity model
- Update route in `src/App.tsx` from `/telemetry` → `/stats`

## Requirement

### Functional Requirements

#### FR1: Time-Range Filtering
- **FR1.1** The stats page SHALL have a segmented control with three options: Weekly, Monthly, Yearly.
- **FR1.2** Selecting a time range SHALL filter all metric cards and charts to show data only within that range.
- **FR1.3** The default selection SHALL be "Weekly."
- **FR1.4** The selected range SHALL persist across page navigations within the same session.

#### FR2: Unified Activity Tracking
- **FR2.1** Both chat messages sent AND review sessions completed SHALL count as user activity.
- **FR2.2** The streak SHALL be computed from any activity type (chat or review), not just chat.
- **FR2.3** A review session is considered "completed" when the user finishes all due cards OR manually ends the session.
- **FR2.4** Existing chat telemetry data SHALL be migrated to the new activity model on app upgrade.

#### FR3: Daily Active Time
- **FR3.1** The app SHALL track total active time per day in minutes.
- **FR3.2** Active time SHALL be recorded while the app is in the foreground and the user is on any learning page (Chat, Review, WordsList).
- **FR3.3** The stats page SHALL display daily active time as a prominent metric card showing:
  - Today's active time (large number, in minutes)
  - Average daily active time for the selected period
  - A trend indicator (up/down arrow vs. previous period)
- **FR3.4** Active time SHALL be displayed in a human-readable format (e.g., "1h 23m" or "45m").

#### FR4: Streak Display
- **FR4.1** The streak SHALL be displayed as a large, prominent metric at the top of the stats page.
- **FR4.2** The streak card SHALL show:
  - Current streak count (number of consecutive days with activity)
  - Longest streak all-time
  - A 7-day mini calendar heatmap showing which days had activity
- **FR4.3** If the user has no activity today, the streak SHALL still count from yesterday (grace period).

#### FR5: Activity Distribution Chart
- **FR5.1** The stats page SHALL include a bar chart showing daily activity counts for the selected period.
- **FR5.2** The chart SHALL differentiate chat activity from review activity using distinct colors.
- **FR5.3** Users SHALL be able to hover over bars to see exact counts per day.
- **FR5.4** The chart SHALL show a trend line (moving average) across the period.

#### FR6: Mastery Progress
- **FR6.1** The stats page SHALL show total mastered words (SRS cards with interval ≥ 21 days).
- **FR6.2** A line chart SHALL show mastery growth over the selected time range.
- **FR6.3** Users SHALL be able to tap/click the mastery card to expand the detailed chart.

#### FR7: Chat Proficiency
- **FR7.1** The stats page SHALL show average chat proficiency score for the selected period.
- **FR7.2** A line chart SHALL plot daily average proficiency scores over time.
- **FR7.3** The chart SHALL include a reference line showing the overall average.

#### FR8: Review Breakdown
- **FR8.1** The stats page SHALL show the distribution of SRS card states (New, Learning, Review, Relearning).
- **FR8.2** The distribution SHALL be displayed as a horizontal stacked bar or donut chart.
- **FR8.3** The page SHALL show the count of cards due for review today.
- **FR8.4** The page SHALL show the count of lapsed cards (cards that moved from Review back to Relearning).

#### FR9: Total Sessions
- **FR9.1** The stats page SHALL display total session count for the selected period.
- **FR9.2** Sessions SHALL be broken down by type: chat sessions vs. review sessions.

### Non-Functional Requirements

#### NFR1: Performance
- **NFR1.1** The stats page SHALL load and render within 1 second on devices with up to 1 year of data.
- **NFR1.2** Chart rendering SHALL not block the main thread; use lazy loading for chart components.
- **NFR1.3** Active time heartbeat SHALL not cause noticeable battery drain (30-second interval max).

#### NFR2: Data Integrity
- **NFR2.1** Migration from `telemetry_days` to `user_activity` SHALL be atomic (all-or-nothing).
- **NFR2.2** Active time SHALL be capped at 16 hours per day to prevent unrealistic values from app left open overnight.
- **NFR2.3** Duplicate activity records for the same date and type SHALL be prevented at the database level.

#### NFR3: UI/UX
- **NFR3.1** The stats page SHALL be responsive and usable on both desktop (Tauri) and mobile (Capacitor) viewports.
- **NFR3.2** All charts SHALL support dark mode via the app's existing theme system.
- **NFR3.3** Metric cards SHALL include subtle entrance animations (fade-in on scroll).
- **NFR3.4** Empty states SHALL be handled gracefully — show "No data yet" with a call-to-action to start learning.

#### NFR4: Backward Compatibility
- **NFR4.1** The old `/telemetry` route SHALL redirect to `/stats` for bookmarked users.
- **NFR4.2** The Home page mini-stats SHALL continue to work with the new data model.
