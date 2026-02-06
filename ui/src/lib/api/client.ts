/**
 * GraphQL Client for Docktail Cluster API
 * 
 * This module provides the base GraphQL client for making queries
 * to the Docktail cluster.
 * 
 * @module api/client
 */

import { GraphQLError } from './errors';

/** GraphQL HTTP endpoint */
// In development, the UI usually runs on 5173/3000 but the API on 8080
// If we are on localhost but not on port 8080, we assume the API is on 8080
const GRAPHQL_ENDPOINT = (() => {
  if (typeof window === 'undefined') return 'http://localhost:8080/graphql';
  
  const { hostname, port, protocol } = window.location;
  
  // If we're on localhost but not on the standard API port, use 8080
  if (hostname === 'localhost' && port !== '8080') {
    return 'http://localhost:8080/graphql';
  }
  
  // Otherwise use the current host (works for production/containers)
  return `${protocol}//${window.location.host}/graphql`;
})();

/**
 * Execute a GraphQL query
 * 
 * Makes an HTTP POST request to the GraphQL endpoint and handles errors.
 * 
 * @param query - GraphQL query string
 * @param variables - Optional query variables
 * @returns Query result data
 * @throws {GraphQLError} If the query fails or returns errors
 * 
 * @example
 * ```typescript
 * const data = await query<{ agents: Agent[] }>(`
 *   query GetAgents {
 *     agents {
 *       id
 *       name
 *     }
 *   }
 * `);
 * ```
 */
export async function query<T>(query: string, variables?: Record<string, any>): Promise<T> {
  const response = await fetch(GRAPHQL_ENDPOINT, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ query, variables }),
  });

  const result = await response.json();
  
  if (result.errors && result.errors.length > 0) {
    const firstError = result.errors[0];
    const errorCode = firstError.extensions?.code;
    const errorMessage = firstError.message;
    
    throw new GraphQLError(errorMessage, errorCode, firstError);
  }

  return result.data;
}
