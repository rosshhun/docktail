/**
 * GraphQL API Helper Functions
 * 
 * This module contains utility functions for working with GraphQL data,
 * including Docker Compose metadata extraction and container grouping.
 * 
 * @module api/helpers
 */

import type { ContainerWithAgent, ComposeMetadata } from './types';

/**
 * Extract Docker Compose metadata from container labels
 * 
 * Parses standard Docker Compose labels to extract project information.
 * Returns null if the container is not part of a Docker Compose project.
 * 
 * @param labels - Container labels array
 * @returns Compose metadata or null if not a compose container
 * 
 * @example
 * ```typescript
 * const container = await fetchContainer('abc123');
 * const compose = extractComposeMetadata(container.labels);
 * if (compose) {
 *   console.log(`Project: ${compose.project}, Service: ${compose.service}`);
 * }
 * ```
 */
export function extractComposeMetadata(labels: Array<{ key: string; value: string }>): ComposeMetadata | null {
  const labelMap = new Map(labels.map(l => [l.key, l.value]));
  
  const project = labelMap.get('com.docker.compose.project');
  if (!project) return null;
  
  return {
    project,
    service: labelMap.get('com.docker.compose.service'),
    version: labelMap.get('com.docker.compose.version'),
    configFiles: labelMap.get('com.docker.compose.project.config_files'),
    containerNumber: labelMap.get('com.docker.compose.container-number'),
    oneoff: labelMap.get('com.docker.compose.oneoff'),
  };
}

/**
 * Group containers by Docker Compose project
 * 
 * Organizes containers into groups based on their Docker Compose project name.
 * Containers without Compose labels are grouped under a special "__ungrouped__" key.
 * 
 * @param containers - Array of containers to group
 * @returns Map of project names to container arrays
 * 
 * @example
 * ```typescript
 * const containers = await fetchContainers();
 * const groups = groupContainersByCompose(containers);
 * 
 * for (const [project, projectContainers] of groups) {
 *   if (project === '__ungrouped__') {
 *     console.log('Standalone containers:', projectContainers.length);
 *   } else {
 *     console.log(`Project ${project}:`, projectContainers.length);
 *   }
 * }
 * ```
 */
export function groupContainersByCompose(containers: ContainerWithAgent[]): Map<string, ContainerWithAgent[]> {
  const groups = new Map<string, ContainerWithAgent[]>();
  const ungrouped: ContainerWithAgent[] = [];
  
  for (const container of containers) {
    const compose = extractComposeMetadata(container.labels);
    if (compose?.project) {
      const existing = groups.get(compose.project) || [];
      existing.push(container);
      groups.set(compose.project, existing);
    } else {
      ungrouped.push(container);
    }
  }
  
  // Add ungrouped containers under a special key
  if (ungrouped.length > 0) {
    groups.set('__ungrouped__', ungrouped);
  }
  
  return groups;
}
