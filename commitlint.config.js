export default {
  extends: ["@commitlint/config-conventional"],
  rules: {
    // 允许的类型：feat/fix/docs/style/refactor/perf/test/build/ci/chore/revert
    // 主题（冒号后正文）长度放宽到 100，避免长描述被拦
    "subject-max-length": [1, "always", 100],
    // 允许大写开头，方便中英混排
    "subject-case": [0],
    "header-max-length": [1, "always", 120],
  },
};
