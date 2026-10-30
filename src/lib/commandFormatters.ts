import { rcuInfoFormatter, type CommandFormatter } from "./rcuInfo.ts";

export type { CommandFormatter };

/**
 * 命令 → 输出格式化器，按命令精确匹配；新命令单独在这里注册，互不影响。
 */
export const COMMAND_FORMATTERS: Record<string, CommandFormatter> = {
  "get-rcu-info": rcuInfoFormatter,
};
