<script setup lang="ts">
import { computed } from "vue";
import { statusText, statusTone } from "@/composables/useFormat";

const props = withDefaults(
  defineProps<{ status: string | null | undefined; label?: string; plain?: boolean }>(),
  { label: undefined, plain: false },
);

const tone = computed(() => statusTone(props.status));
const text = computed(() => props.label ?? statusText(props.status));
</script>

<template>
  <span class="badge" :class="[tone, { plain }]">{{ text }}</span>
</template>
