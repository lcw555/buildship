import { createRouter, createWebHashHistory } from "vue-router";

/** 使用 hash 路由，避免打包后 file:// 协议下的路径问题 */
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    {
      path: "/",
      name: "dashboard",
      component: () => import("@/views/DashboardView.vue"),
      meta: { title: "工作台", subtitle: "最近项目、构建任务与线上版本概览", icon: "dashboard" },
    },
    {
      path: "/projects",
      name: "projects",
      component: () => import("@/views/ProjectsView.vue"),
      meta: { title: "项目", subtitle: "添加本地前端项目、识别包管理器与构建配置", icon: "projects" },
    },
    {
      path: "/builds",
      name: "builds",
      component: () => import("@/views/BuildsView.vue"),
      meta: { title: "构建任务", subtitle: "执行构建、查看实时日志与历史结果", icon: "builds" },
    },
    {
      path: "/artifacts",
      name: "artifacts",
      component: () => import("@/views/ArtifactsView.vue"),
      meta: { title: "发布包", subtitle: "ZIP 归档、manifest 与 SHA-256 校验", icon: "artifacts" },
    },
    {
      path: "/releases",
      name: "releases",
      component: () => import("@/views/ReleasesView.vue"),
      meta: { title: "发布管理", subtitle: "部署环境、IIS 站点、版本历史与回滚", icon: "releases" },
    },
    {
      path: "/audit",
      name: "audit",
      component: () => import("@/views/AuditView.vue"),
      meta: { title: "操作审计", subtitle: "构建、打包、发布、回滚的不可变记录", icon: "audit" },
    },
    {
      path: "/settings",
      name: "settings",
      component: () => import("@/views/SettingsView.vue"),
      meta: { title: "设置", subtitle: "工具链体检、启动行为与数据备份", icon: "settings" },
    },
  ],
});

export default router;
