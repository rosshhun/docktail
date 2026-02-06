// Container status parsing utilities

/**
 * Parse uptime from Docker status field
 */
export function parseUptime(status: string, state: string): string {
  if (state === 'RUNNING') {
    // Parse running status like "Up 2 hours" or "Up About a minute" or "Up 3 days"
    // Remove health status like "(healthy)" or "(unhealthy)" from the string
    const upMatch = status.match(/Up\s+(.+?)(?:\s*\([^)]*\))?(?:,|$)/i);
    if (upMatch) {
      let uptime = upMatch[1].trim();
      // Remove any remaining parenthetical content
      uptime = uptime.replace(/\s*\([^)]*\)/g, '');
      // Simplify the format - ORDER MATTERS!
      // 1. First replace "a/an" BEFORE replacing time units
      // Note: \s* means zero or more spaces (to handle both "an hour" and " an hour")
      return uptime
        .replace(/About\s+/i, '')
        .replace(/Less than\s+/i, '< ')
        .replace(/\s*a\s+second/i, '1 second')
        .replace(/\s*an?\s+minute/i, '1 minute')
        .replace(/\s*an?\s+hour/i, '1 hour')
        .replace(/\s*a\s+day/i, '1 day')
        .replace(/\s*a\s+week/i, '1 week')
        .replace(/\s*a\s+month/i, '1 month')
        // 2. Now replace time units with abbreviations
        .replace(/\s*seconds?/i, 's')
        .replace(/\s*minutes?/i, 'm')
        .replace(/\s*hours?/i, 'h')
        .replace(/\s*days?/i, 'd')
        .replace(/\s*weeks?/i, 'w')
        .replace(/\s*months?/i, 'mo');
    }
    return 'just started';
  } else if (state === 'EXITED' || state === 'DEAD') {
    // Parse exited status like "Exited (137) 2 hours ago"
    const exitMatch = status.match(/Exited.*?(\d+\s+\w+)\s+ago/i);
    if (exitMatch) {
      const time = exitMatch[1]
        .replace(/\s+seconds?/i, 's')
        .replace(/\s+minutes?/i, 'm')
        .replace(/\s+hours?/i, 'h')
        .replace(/\s+days?/i, 'd')
        .replace(/\s+weeks?/i, 'w')
        .replace(/\s+months?/i, 'mo');
      return `${time} ago`;
    }
    // Fallback: check if it just says "Exited (code)" without time
    if (status.match(/Exited\s*\(\d+\)/i)) {
      return 'just now';
    }
  } else if (state === 'CREATED') {
    return 'not started';
  } else if (state === 'PAUSED') {
    return 'paused';
  } else if (state === 'RESTARTING') {
    return 'restarting';
  }
  
  return '-';
}

/**
 * Extract exit code from container status
 */
export function parseExitCode(status: string, state: string): number | null {
  if (state !== 'EXITED' && state !== 'DEAD') return null;
  
  // Parse status like "Exited (137) 2 hours ago" or "Exited (0)"
  const exitMatch = status.match(/Exited \((\d+)\)/);
  if (exitMatch) {
    return parseInt(exitMatch[1]);
  }
  return null;
}
