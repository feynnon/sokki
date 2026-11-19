/**
 * Format a date string or timestamp to a localized date and time string
 * @param timestamp - Unix timestamp in seconds (as string)
 * @param locale - BCP 47 language tag (e.g., 'en', 'es', 'fr')
 * @returns Formatted date string
 */
export const formatDateTime = (timestamp: string, locale: string): string => {
  try {
    // Convert Unix timestamp (seconds) to milliseconds
    const timestampMs = parseInt(timestamp, 10) * 1000;
    const date = new Date(timestampMs);

    // Check if date is valid
    if (isNaN(date.getTime())) {
      return timestamp; // Return original if invalid
    }

    return new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "long",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(date);
  } catch (error) {
    console.error("Failed to format date:", error);
    return timestamp; // Fallback to original timestamp
  }
};

/**
 * Format a date string or timestamp to a localized date string (no time)
 * @param timestamp - Unix timestamp in seconds (as string)
 * @param locale - BCP 47 language tag (e.g., 'en', 'es', 'fr')
 * @returns Formatted date string
 */
export const formatDate = (timestamp: string, locale: string): string => {
  try {
    // Convert Unix timestamp (seconds) to milliseconds
    const timestampMs = parseInt(timestamp, 10) * 1000;
    const date = new Date(timestampMs);

    // Check if date is valid
    if (isNaN(date.getTime())) {
      return timestamp; // Return original if invalid
    }

    return new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "long",
      day: "numeric",
    }).format(date);
  } catch (error) {
    console.error("Failed to format date:", error);
    return timestamp; // Fallback to original timestamp
  }
};

/**
 * Format a date string or timestamp to a relative time string (e.g., "2 hours ago")
 * @param timestamp - Unix timestamp in seconds (as string)
 * @param locale - BCP 47 language tag (e.g., 'en', 'es', 'fr')
 * @returns Relative time string
 */
export const formatRelativeTime = (
  timestamp: string,
  locale: string,
): string => {
  try {
    // Convert Unix timestamp (seconds) to milliseconds
    const timestampMs = parseInt(timestamp, 10) * 1000;
    const date = new Date(timestampMs);
    const now = new Date();

    // Check if date is valid
    if (isNaN(date.getTime())) {
      return timestamp; // Return original if invalid
    }

    const diffInSeconds = Math.floor((now.getTime() - date.getTime()) / 1000);

    // Use Intl.RelativeTimeFormat for proper localization
    const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });

    // Less than a minute
    if (diffInSeconds < 60) {
      return rtf.format(-diffInSeconds, "second");
    }

    // Less than an hour
    const diffInMinutes = Math.floor(diffInSeconds / 60);
    if (diffInMinutes < 60) {
      return rtf.format(-diffInMinutes, "minute");
    }

    // Less than a day
    const diffInHours = Math.floor(diffInMinutes / 60);
    if (diffInHours < 24) {
      return rtf.format(-diffInHours, "hour");
    }

    // Less than a week
    const diffInDays = Math.floor(diffInHours / 24);
    if (diffInDays < 7) {
      return rtf.format(-diffInDays, "day");
    }

    // Less than a month (30 days)
    if (diffInDays < 30) {
      const diffInWeeks = Math.floor(diffInDays / 7);
      return rtf.format(-diffInWeeks, "week");
    }

    // Less than a year
    if (diffInDays < 365) {
      const diffInMonths = Math.floor(diffInDays / 30);
      return rtf.format(-diffInMonths, "month");
    }

    // More than a year
    const diffInYears = Math.floor(diffInDays / 365);
    return rtf.format(-diffInYears, "year");
  } catch (error) {
    console.error("Failed to format relative time:", error);
    return formatDateTime(timestamp, locale); // Fallback to absolute time
  }
};

/**
 * Short date and time for lists: "Today, 09:25", "Yesterday, 17:08",
 * "18 Nov, 17:08", "18 Nov 2025, 17:08". Words in the app's language; the
 * clock (12 or 24 hours) follows the system.
 * @param timestamp - Unix timestamp in seconds (as string)
 */
export const formatShortDateTime = (
  timestamp: string,
  locale: string,
): string => {
  const date = new Date(parseInt(timestamp, 10) * 1000);
  if (isNaN(date.getTime())) return timestamp;
  try {
    const hourCycle = new Intl.DateTimeFormat(
      typeof navigator !== "undefined" ? navigator.language : locale,
      { hour: "numeric" },
    ).resolvedOptions().hourCycle;
    const time = new Intl.DateTimeFormat(locale, {
      hour: "2-digit",
      minute: "2-digit",
      hourCycle,
    }).format(date);
    const startOf = (d: Date) =>
      new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const days = Math.round((startOf(new Date()) - startOf(date)) / 86_400_000);
    let day: string;
    if (days === 0 || days === 1) {
      const rel = new Intl.RelativeTimeFormat(locale, {
        numeric: "auto",
      }).format(-days, "day");
      day = rel.charAt(0).toLocaleUpperCase(locale) + rel.slice(1);
    } else {
      day = new Intl.DateTimeFormat(locale, {
        day: "numeric",
        month: "short",
        year:
          date.getFullYear() === new Date().getFullYear()
            ? undefined
            : "numeric",
      }).format(date);
    }
    return `${day}, ${time}`;
  } catch {
    return formatDateTime(timestamp, locale);
  }
};
