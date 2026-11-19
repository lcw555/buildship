<script setup lang="ts">
import { NConfigProvider, NDialogProvider, NMessageProvider, zhCN, dateZhCN } from "naive-ui";
import type { GlobalThemeOverrides } from "naive-ui";
import AppShell from "@/components/AppShell.vue";

/** 品牌色与方案 6.1 的 Light Blue 视觉方向保持一致 */
const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#2f6bff",
    primaryColorHover: "#1f57e0",
    primaryColorPressed: "#1a4bc4",
    primaryColorSuppl: "#2f6bff",
    borderRadius: "10px",
    borderRadiusSmall: "8px",
    fontFamily:
      'Inter, "PingFang SC", "Microsoft YaHei", "Segoe UI", system-ui, -apple-system, sans-serif',
    textColorBase: "#16233a",
    bodyColor: "#f4f8fe",
  },
  Button: {
    fontWeight: "550",
  },
  Input: {
    borderRadius: "10px",
  },
  Card: {
    borderRadius: "16px",
  },
  Modal: {
    borderRadius: "16px",
  },
};
</script>

<template>
  <n-config-provider :theme-overrides="themeOverrides" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider :max="4" placement="top-right">
      <n-dialog-provider>
        <AppShell />
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
