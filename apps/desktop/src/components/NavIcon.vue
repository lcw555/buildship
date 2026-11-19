<script setup lang="ts">
const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 18 });

const PATHS: Record<string, string> = {
  dashboard: "M3 12h6V3H3v9Zm0 9h6v-6H3v6Zm9 0h9v-9h-9v9Zm0-18v6h9V3h-9Z",
  projects: "M3 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z",
  builds: "M4 4h10l6 6v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Zm10 1v6h6M7 14h8M7 17h5",
  artifacts: "M12 3 3 7.5v9L12 21l9-4.5v-9L12 3Zm0 0v18M3 7.5 12 12l9-4.5",
  releases: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18Zm0 4v5l4 2",
  audit: "M6 3h9l4 4v14H6V3Zm4 8h6M10 15h6M10 7h3",
  settings:
    "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm8 3-.2-1.6 1.6-1.3-1.6-2.7-1.9.7-1.4-.8-.3-2h-3.1l-.3 2-1.4.8-1.9-.7L4.6 8.1 6.2 9.4 6 11l-1.6 1.3 1.6 2.7 1.9-.7 1.4.8.3 2h3.1l.3-2 1.4-.8 1.9.7 1.6-2.7L18.4 12Z",
};

const path = PATHS[props.name] ?? PATHS.dashboard;
</script>

<template>
  <svg
    class="nav-icon"
    :width="size"
    :height="size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.6"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path :d="path" />
  </svg>
</template>
