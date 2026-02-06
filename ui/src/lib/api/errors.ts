/**
 * GraphQL Error Handling
 * 
 * This module defines error classes and utilities for handling GraphQL errors
 * from the Docktail cluster API.
 * 
 * @module api/errors
 */

/**
 * GraphQL Error with structured error codes
 * 
 * Extends the standard Error class with error codes from the backend
 * and provides helper methods for error classification and user messages.
 */
export class GraphQLError extends Error {
  /** Error code from backend (e.g., 'CONTAINER_NOT_FOUND') */
  code?: string;
  /** Original error object from GraphQL response */
  originalError: any;

  constructor(message: string, code?: string, originalError?: any) {
    super(message);
    this.name = 'GraphQLError';
    this.code = code;
    this.originalError = originalError;
  }

  // ============================================================================
  // Error Type Checks
  // ============================================================================

  /**
   * Check if error is a "not found" error
   * @returns True if container or agent was not found
   */
  isNotFound(): boolean {
    return this.code === 'CONTAINER_NOT_FOUND' || this.code === 'AGENT_NOT_FOUND';
  }

  /**
   * Check if error is due to agent unavailability
   * @returns True if agent is currently unavailable
   */
  isUnavailable(): boolean {
    return this.code === 'AGENT_UNAVAILABLE';
  }

  /**
   * Check if error is an authentication error
   * @returns True if authentication is required
   */
  isUnauthorized(): boolean {
    return this.code === 'UNAUTHORIZED';
  }

  /**
   * Check if error is a permission error
   * @returns True if user lacks permission
   */
  isForbidden(): boolean {
    return this.code === 'FORBIDDEN';
  }

  /**
   * Check if error is an internal server error
   * @returns True if this is a server-side error
   */
  isInternalError(): boolean {
    return this.code === 'INTERNAL_SERVER_ERROR' || this.code === 'GRPC_ERROR';
  }

  /**
   * Check if error is a bad request error
   * @returns True if the request was malformed
   */
  isBadRequest(): boolean {
    return this.code === 'BAD_REQUEST';
  }

  // ============================================================================
  // User-Friendly Messages
  // ============================================================================

  /**
   * Get a user-friendly error message based on error code
   * @returns Human-readable error message suitable for display
   */
  getUserMessage(): string {
    switch (this.code) {
      case 'CONTAINER_NOT_FOUND':
        return 'Container not found or no longer exists';
      case 'AGENT_NOT_FOUND':
        return 'Agent not found or disconnected';
      case 'AGENT_UNAVAILABLE':
        return 'Agent is currently unavailable. Please try again later';
      case 'UNAUTHORIZED':
        return 'Authentication required';
      case 'FORBIDDEN':
        return 'You do not have permission to access this resource';
      case 'BAD_REQUEST':
        return 'Invalid request. Please check your input';
      case 'INTERNAL_SERVER_ERROR':
        return 'An unexpected error occurred. Please try again';
      case 'GRPC_ERROR':
        return 'Communication error with agent. Please retry';
      case 'WEBSOCKET_ERROR':
        return 'WebSocket connection error. Check your network connection';
      default:
        return this.message;
    }
  }

  /**
   * Check if this error is retryable
   * 
   * Some errors are transient and may succeed on retry (e.g., agent
   * temporarily unavailable), while others are permanent (e.g., not found).
   * 
   * @returns True if retrying the request may succeed
   */
  isRetryable(): boolean {
    return this.code === 'AGENT_UNAVAILABLE' || 
           this.code === 'GRPC_ERROR' || 
           this.code === 'INTERNAL_SERVER_ERROR' ||
           this.code === 'WEBSOCKET_ERROR';
  }
}
