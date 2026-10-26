import { t } from '../../shared/i18n';
import { useServiceStatus } from './useServiceStatus';

export function ServiceStatus() {
  const { service, retry } = useServiceStatus();
  return (
    <section className="service" aria-label={t("服务连接状态")}>
      <div role="status" aria-live="polite">
        <span className={`status-dot ${service.status}`} aria-hidden="true" />
        <strong>{service.status === 'loading' ? t("正在连接服务…") : service.status === 'online' ? t("后端已连接") : t("暂时无法连接后端")}</strong>
        <p>{service.status === 'online' ? `Synquay v${service.version} · 基础服务运行正常` : service.status === 'offline' ? t("请确认后端正在运行，然后重新检查。") : t("正在检查服务是否可用。")}</p>
        {service.status === 'offline' && service.requestId && <p>{t("排查编号：")}{service.requestId}</p>}
      </div>
      <button disabled={service.status === 'loading'} onClick={retry}>{t("重新检查")}</button>
    </section>
  );
}
