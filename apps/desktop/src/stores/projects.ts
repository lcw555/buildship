import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { call } from "@/composables/useIpc";
import type { AddProjectInput, BuildProfile, Project, ScanResult, UpdateProjectInput } from "@/types/ipc";

/** 项目与默认构建配置 */
export const useProjectsStore = defineStore("projects", () => {
  const items = ref<Project[]>([]);
  const loading = ref(false);
  const currentId = ref<string | null>(null);
  const profiles = ref<Record<string, BuildProfile[]>>({});
  const scanResult = ref<ScanResult | null>(null);

  const current = computed<Project | null>(
    () => items.value.find((item) => item.id === currentId.value) ?? items.value[0] ?? null,
  );

  async function load() {
    loading.value = true;
    try {
      items.value = await call<Project[]>("project_list", { status: "active" });
      if (!currentId.value && items.value.length > 0) {
        currentId.value = items.value[0].id;
      }
      if (currentId.value && !items.value.some((item) => item.id === currentId.value)) {
        currentId.value = items.value[0]?.id ?? null;
      }
    } finally {
      loading.value = false;
    }
  }

  function select(id: string) {
    currentId.value = id;
    void call("quick_set_project", { projectId: id }).catch(() => undefined);
  }

  async function scan(path: string) {
    scanResult.value = await call<ScanResult>("project_scan", { path });
    return scanResult.value;
  }

  async function add(input: AddProjectInput) {
    const project = await call<Project>("project_add", { input });
    await load();
    currentId.value = project.id;
    return project;
  }

  async function update(input: UpdateProjectInput) {
    const project = await call<Project>("project_update", { input });
    await load();
    profiles.value = {};
    return project;
  }

  async function archive(id: string) {
    await call("project_archive", { id });
    await load();
  }

  async function loadProfiles(projectId: string) {
    profiles.value[projectId] = await call<BuildProfile[]>("project_profiles", { projectId });
    return profiles.value[projectId];
  }

  return {
    items,
    loading,
    currentId,
    current,
    profiles,
    scanResult,
    load,
    select,
    scan,
    add,
    update,
    archive,
    loadProfiles,
  };
});
