import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { call, isTauri } from "@/composables/useIpc";
import type {
  DeployEvent,
  EnvironmentWithTargets,
  Release,
  ReleaseDetail,
  SaveEnvironmentInput,
  SaveTargetInput,
  DeploymentTarget,
} from "@/types/ipc";

/** 环境 / 目标 / 发布 / 回滚 */
export const useReleasesStore = defineStore("releases", () => {
  const environments = ref<EnvironmentWithTargets[]>([]);
  const releases = ref<Release[]>([]);
  const detail = ref<ReleaseDetail | null>(null);
  const loading = ref(false);
  const deploying = ref(false);
  const progress = ref(0);
  const stage = ref<string | null>(null);
  const bridgeReady = ref(false);

  const targets = computed<DeploymentTarget[]>(() =>
    environments.value.flatMap((item) => item.targets),
  );

  const currentRelease = computed<Release | null>(
    () => releases.value.find((item) => item.isCurrent) ?? null,
  );

  async function loadEnvironments() {
    environments.value = await call<EnvironmentWithTargets[]>("env_list");
    return environments.value;
  }

  async function saveEnvironment(input: SaveEnvironmentInput) {
    const result = await call("env_save", { input });
    await loadEnvironments();
    return result;
  }

  async function archiveEnvironment(id: string) {
    await call("env_archive", { id });
    await loadEnvironments();
  }

  async function saveTarget(input: SaveTargetInput) {
    await call("target_save", { input });
    await loadEnvironments();
  }

  async function archiveTarget(id: string) {
    await call("target_archive", { id });
    await loadEnvironments();
  }

  async function checkTarget(id: string) {
    const result = await call<{ ok: boolean; message: string }>("target_check", { id });
    await loadEnvironments();
    return result;
  }

  async function load(projectId?: string, environmentId?: string, limit = 100) {
    loading.value = true;
    try {
      releases.value = await call<Release[]>("release_list", {
        projectId: projectId ?? null,
        environmentId: environmentId ?? null,
        limit,
      });
    } finally {
      loading.value = false;
    }
  }

  async function loadDetail(id: string) {
    detail.value = await call<ReleaseDetail>("release_detail", { id });
    return detail.value;
  }

  async function create(artifactId: string, targetId: string, version?: string) {
    deploying.value = true;
    progress.value = 0;
    stage.value = "准备中";
    try {
      const release = await call<Release>("release_create", {
        input: { artifactId, targetId, version: version ?? null },
      });
      await load();
      return release;
    } finally {
      deploying.value = false;
    }
  }

  async function rollback(targetReleaseId: string, reason: string) {
    deploying.value = true;
    progress.value = 0;
    stage.value = "回滚中";
    try {
      return await call<Release>("release_rollback", {
        input: { targetReleaseId, reason },
      });
    } finally {
      deploying.value = false;
    }
  }

  async function healthyCandidates(projectId: string, environmentId: string) {
    const list = await call<Release[]>("release_healthy_candidates", { projectId, environmentId });
    return list;
  }

  function handleEvent(payload: DeployEvent) {
    progress.value = payload.progress;
    stage.value = payload.stage;
    const index = releases.value.findIndex((item) => item.id === payload.releaseId);
    if (index >= 0) {
      releases.value[index] = {
        ...releases.value[index],
        status: payload.releaseStatus,
      };
    }
    if (
      payload.status === "succeeded" ||
      payload.status === "failed" ||
      payload.status === "cancelled"
    ) {
      deploying.value = false;
      void load().catch(() => undefined);
    }
  }

  async function ensureBridge() {
    if (bridgeReady.value || !isTauri()) return;
    const { listen } = await import("@tauri-apps/api/event");
    await listen<DeployEvent>("deploy://status", (event) => handleEvent(event.payload));
    bridgeReady.value = true;
  }

  return {
    environments,
    releases,
    detail,
    loading,
    deploying,
    progress,
    stage,
    targets,
    currentRelease,
    loadEnvironments,
    saveEnvironment,
    archiveEnvironment,
    saveTarget,
    archiveTarget,
    checkTarget,
    load,
    loadDetail,
    create,
    rollback,
    healthyCandidates,
    handleEvent,
    ensureBridge,
  };
});
