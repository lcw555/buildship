<script setup lang="ts">
withDefaults(defineProps<{ title: string; desc?: string }>(), { desc: "" });
</script>

<template>
  <div class="empty-state">
    <svg width="42" height="42" viewBox="0 0 48 48" fill="none" aria-hidden="true">
      <rect x="6" y="10" width="36" height="28" rx="6" stroke="#c6d7ee" stroke-width="2" />
      <path d="M13 24h22M13 30h14" stroke="#dbe6f6" stroke-width="2" stroke-linecap="round" />
      <circle cx="34" cy="17" r="3" fill="#e8f0ff" />
    </svg>
    <div class="empty-title">{{ title }}</div>
    <div v-if="desc" class="empty-desc">{{ desc }}</div>
    <div class="empty-action">
      <slot />
    </div>
  </div>
</template>

<style scoped>
.empty-action {
  margin-top: 14px;
  display: flex;
  justify-content: center;
  gap: 8px;
}
</style>
