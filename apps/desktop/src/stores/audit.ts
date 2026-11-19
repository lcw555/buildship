import { defineStore } from "pinia";
import { ref } from "vue";
import { call } from "@/composables/useIpc";
import type { AuditEvent } from "@/types/ipc";

/** 操作审计（只读） */
export const useAuditStore = defineStore("audit", () => {
  const items = ref<AuditEvent[]>([]);
  const total = ref(0);
  const loading = ref(false);

  async function load(options: { projectId?: string | null; action?: string | null; limit?: number; offset?: number } = {}) {
    loading.value = true;
    try {
      const page = await call<{ items: AuditEvent[]; total: number }>("audit_list", {
        query: {
          projectId: options.projectId ?? null,
          action: options.action ?? null,
          limit: options.limit ?? 200,
          offset: options.offset ?? 0,
        },
      });
      items.value = page.items;
      total.value = page.total;
    } finally {
      loading.value = false;
    }
  }

  return { items, total, loading, load };
});
