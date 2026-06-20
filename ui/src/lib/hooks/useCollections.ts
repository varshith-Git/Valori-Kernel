"use client";

import useSWR from "swr";
import { useProjects } from "./useProjects";

const SEP = "--";

export const makeNs = (project: string, collection: string) =>
  `${project}${SEP}${collection}`;

export const parseNs = (
  ns: string
): { project: string; collection: string } => {
  const idx = ns.indexOf(SEP);
  if (idx === -1) return { project: ns, collection: ns };
  return {
    project: ns.slice(0, idx),
    collection: ns.slice(idx + SEP.length),
  };
};

export interface ProjectGroup {
  project: string;
  collections: string[];
  isBare: boolean; // true when the namespace IS the project (no "--")
}

export function useProjectGroups(): {
  groups: ProjectGroup[];
  isLoading: boolean;
  error: unknown;
  refresh: () => void;
} {
  const { projects: namespaces, isLoading, error, refresh } = useProjects();

  const grouped = new Map<string, ProjectGroup>();

  for (const ns of namespaces) {
    const { project, collection } = parseNs(ns);
    const existing = grouped.get(project);
    if (!existing) {
      const isBare = !ns.includes(SEP);
      grouped.set(project, {
        project,
        collections: isBare ? [] : [collection],
        isBare,
      });
    } else {
      if (!existing.isBare) existing.collections.push(collection);
    }
  }

  return {
    groups: Array.from(grouped.values()),
    isLoading,
    error,
    refresh,
  };
}

export function useCollections(project: string) {
  const { projects: namespaces, isLoading, error, refresh } = useProjects();

  const collections = namespaces
    .filter((ns) => ns.startsWith(`${project}${SEP}`))
    .map((ns) => ns.slice(project.length + SEP.length));

  const create = async (collection: string) => {
    const ns = makeNs(project, collection);
    await fetch("/api/namespaces", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: ns }),
    });
    refresh();
  };

  const drop = async (collection: string) => {
    const ns = makeNs(project, collection);
    await fetch(`/api/namespaces/${encodeURIComponent(ns)}`, {
      method: "DELETE",
    });
    refresh();
  };

  return { collections, isLoading, error, create, drop, refresh };
}
