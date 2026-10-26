import { t } from '../../shared/i18n';

// Localize known event labels for display; routing still uses the original identifier.
const eventLabels: Readonly<Record<string, string>> = {
  'device.command': '设备指令',
  'device.status': '设备状态',
  'device.alert': '设备告警',
  'device.alert.night': '夜间设备告警',
  'app.message': '应用消息',
};

export function eventTypeLabel(eventType: string): string {
  return Object.hasOwn(eventLabels, eventType) ? t(eventLabels[eventType]) : eventType;
}
