<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { NButton, NDrawer, NDrawerContent, NSelect } from "naive-ui";
import EmptyState from "@/components/EmptyState.vue";
import { errorText } from "@/composables/useIpc";
import { actionText, formatTime } from "@/composables/useFormat";
import { useAuditStore } from "@/stores/audit";
import { useProjectsStore } from "@/stores/projects";
import type { AuditEvent } from "@/types/ipc";

const audit = useAuditStore();
const projects = useProjectsStore();

const projectId = ref<string | null>(null);
const action = ref<string | null>(null);
const detail = ref<AuditEvent | null>(null);
const detailOpen = ref(false);

const projectOptions = computed(() => [
  { label: "全部项目", value: null as unknown as string },
  ...projects.items.map((item) => ({ label: item.name, value: item.id })),
]);

const actionOptions = [
  { label: "全部操作", value: null as unknown as string },
  { label: "添加项目", value: "project_added" },
  { label: "修改项目", value: "project_updated" },
  { label: "归档项目", value: "project_archived" },
  { label: "启动构建", value: "build_started" },
  { label: "构建成功", value: "build_succeeded" },
  { label: "构建失败", value: "build_failed" },
  { label: "取消构建", value: "build_cancelled" },
  { label: "生成发布包", value: "artifact_created" },
  { label: "校验发布包", value: "artifact_verified" },
  { label: "删除发布包", value: "artifact_deleted" },
  { label: "创建发布", value: "release_created" },
  { label: "发布成功", value: "release_succeeded" },
  { label: "发布失败", value: "release_failed" },
  { label: "版本回滚", value: "release_rolled_back" },
  { label: "回滚失败", value: "rollback_failed" },
  { label: "创建备份", value: "backup_created" },
  { label: "恢复备份", value: "backup_restored" },
  { label: "修改设置", value: "settings_updated" },
];

onMounted(async () => {
  await projects.load();
  await load();
});

async function load() {
  try {
    await audit.load({ projectId: projectId.value, action: action.value });
  } catch (err) {
    console.error(errorText(err));
  }
}

function openDetail(event: AuditEvent) {
  detail.value = event;
  detailOpen.value = true;
}

function pretty(json: string | null): string {
  if (!json) return "无附加信息";
  try {
    return JSON.stringify(JSON.parse(json), null, 2);
  } catch {
    return json;
  }
}
</script>

<template>
  <div>
    <div class="toolbar">
      <n-select v-model:value="projectId" :options="projectOptions" style="width: 200px" @update:value="load" />
      <n-select v-model:value="action" :options="actionOptions" style="width: 180px" @update:value="load" />
      <n-button quaternary @click="load">刷新</n-button>
      <div class="spacer" />
      <span class="muted" style="font-size: 12px">
        审计记录只允许新增，数据库触发器禁止 UPDATE / DELETE
      </span>
    </div>

    <div class="card">
      <div class="card-head">
        <div>
          <div class="card-title">操作审计</div>
          <div class="card-desc">共 {{ audit.total }} 条记录（显示最近 {{ audit.items.length }} 条）</div>
        </div>
      </div>
      <div class="card-body tight">
        <table v-if="audit.items.length" class="table">
          <thead>
            <tr>
              <th style="width: 160px">时间</th>
              <th style="width: 120px">操作</th>
              <th>摘要</th>
              <th style="width: 120px">操作人</th>
              <th style="width: 90px"></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="event in audit.items" :key="event.id">
              <td class="muted nowrap">{{ formatTime(event.createdAt) }}</td>
              <td>
                <span class="badge plain info">{{ actionText(event.action) }}</span>
              </td>
              <td>{{ event.summary }}</td>
              <td class="muted">{{ event.actor }}</td>
              <td style="text-align: right">
                <n-button size="tiny" quaternary @click="openDetail(event)">详情</n-button>
              </td>
            </tr>
          </tbody>
        </table>
        <EmptyState v-else title="暂无审计记录" desc="添加项目、执行构建或发布后，这里会出现不可篡改的操作记录" />
      </div>
    </div>

    <n-drawer v-model:show="detailOpen" :width="460" placement="right">
      <n-drawer-content title="审计详情" closable>
        <dl v-if="detail" class="kv">
          <dt>记录 ID</dt>
          <dd class="mono">{{ detail.id }}</dd>
          <dt>时间</dt>
          <dd>{{ formatTime(detail.createdAt) }}</dd>
          <dt>操作</dt>
          <dd>{{ actionText(detail.action) }}（{{ detail.action }}）</dd>
          <dt>对象</dt>
          <dd class="mono">{{ detail.entityType }} / {{ detail.entityId }}</dd>
          <dt>项目</dt>
          <dd class="mono">{{ detail.projectId ?? "-" }}</dd>
          <dt>操作人</dt>
          <dd>{{ detail.actor }}</dd>
          <dt>摘要</dt>
          <dd>{{ detail.summary }}</dd>
        </dl>
        <pre
          class="mono"
          style="
            margin-top: 16px;
            padding: 12px;
            background: var(--bs-surface-2);
            border: 1px solid var(--bs-border);
            border-radius: 10px;
            font-size: 12px;
            white-space: pre-wrap;
            word-break: break-all;
          "
          >{{ pretty(detail?.detailJson ?? null) }}</pre
        >
      </n-drawer-content>
    </n-drawer>
  </div>
</template>
