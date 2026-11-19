import { defineStore } from "pinia";
import { ref } from "vue";
import { call } from "@/composables/useIpc";
import type { Artifact, ArtifactVerifyResult } from "@/types/ipc";

/** 发布包 */
export const useArtifactsStore = defineStore("artifacts", () => {
  const items = ref<Artifact[]>([]);
  const loading = ref(false);
  const creating = ref(false);

  async function load(projectId?: string, limit = 100) {
    loading.value = true;
    try {
      items.value = await call<Artifact[]>("artifact_list", {
        projectId: projectId ?? null,
        limit,
      });
    } finally {
      loading.value = false;
    }
  }

  async function create(buildTaskId: string) {
    creating.value = true;
    try {
      const artifact = await call<Artifact>("artifact_create", { buildTaskId });
      items.value.unshift(artifact);
      return artifact;
    } finally {
      creating.value = false;
    }
  }

  async function verify(id: string) {
    return call<ArtifactVerifyResult>("artifact_verify", { id });
  }

  async function openDir(id: string) {
    await call("artifact_open_dir", { id });
  }

  async function remove(id: string) {
    await call("artifact_delete", { id });
    items.value = items.value.filter((item) => item.id !== id);
  }

  return { items, loading, creating, load, create, verify, openDir, remove };
});
