// Built-in console copy only. User content and service payloads are never translated.
export const catalog: Record<string, readonly [string, string, string]> = {
"允许读取此连接的全部近期消息":["Allow reading all recent messages from this connection", "この接続の最近のメッセージの読み取りを許可", "Permitir leer mensajes recientes de esta conexión"],
"启用近期消息池":["Enable recent messages", "最近のメッセージを有効化", "Activar mensajes recientes"],
"消息保存在内存中，重启后清空；不影响持久化事件保留。":["Messages are kept in memory and cleared on restart; persistent event retention is separate.", "メッセージはメモリに保存され再起動時に消去されます。永続イベントの保持とは別です。", "Los mensajes se conservan en memoria y se borra al reiniciar; la retención de eventos persistentes es independiente."],
"最大消息数":["Maximum messages", "最大メッセージ数", "Máximo de mensajes"],
"保留时间（秒）":["Retention (seconds)", "保持期間（秒）", "Retención (segundos)"],
"请输入 1–1000 的整数。":["Enter an integer from 1 to 1000.", "1〜1000 の整数を入力してください。", "Introduce un entero entre 1 y 1000."],
"请输入 1–3600 的整数。":["Enter an integer from 1 to 3600.", "1〜3600 の整数を入力してください。", "Introduce un entero entre 1 y 3600."],
"指令由当前来源已启用的指令规则识别": ["Commands are identified by enabled command rules for this source", "このソースの有効なコマンドルールで識別します", "Los comandos se identifican mediante reglas activas de este origen"],
"消息类型": ["Message types", "メッセージ種別", "Tipos de mensaje"],
"至少选择一种消息类型": ["Choose at least one message type", "メッセージ種別を1つ以上選択してください", "Selecciona al menos un tipo de mensaje"],
"普通消息": ["Messages", "通常メッセージ", "Mensajes"],
"状态上报": ["State reports", "状態報告", "Informes de estado"],
"指令消息": ["Commands", "コマンド", "Comandos"],
"包含状态上报": ["Include state reports", "状態報告を含める", "Incluir informes de estado"],
"内置工具": ["Built-in tools", "組み込みツール", "Herramientas integradas"],
"允许使用的连接": ["Allowed connections", "使用を許可する接続", "Conexiones permitidas"],
"内置查询工具": ["Built-in queries", "組み込み照会", "Consultas integradas"],
"查询连接": ["List connections", "接続一覧", "Listar conexiones"],
"查询连接状态": ["Connection status", "接続状態", "Estado de conexión"],
"查询收发数量": ["Message counts", "送受信数", "Recuento de mensajes"],
"发送 QQ 文本消息": ["Send QQ text", "QQ テキストを送信", "Enviar texto en QQ"],
"选择允许发送的群或单聊": ["Choose groups or direct conversations allowed for sending", "送信を許可するグループ・個人会話を選択", "Selecciona grupos o conversaciones autorizados para enviar"],
"请先在 QQ 连接中启用主动发送。": ["Enable proactive sending in the QQ connection first.", "まず QQ 接続で能動的な送信を有効にしてください。", "Activa primero el envío proactivo en la conexión QQ."],
"查询 QQ 机器人信息": ["QQ bot information", "QQ ボット情報", "Información del bot de QQ"],
"查询近期消息": ["Recent messages", "最近のメッセージ", "Mensajes recientes"],
"允许查询的连接": ["Allowed connections", "許可する接続", "Conexiones permitidas"],
"读取连接失败，请稍后重试。": ["Could not load connections. Try again later.", "接続を読み込めませんでした。後でもう一度お試しください。", "No se pudieron cargar las conexiones. Inténtalo más tarde."],
"暂无可用连接": ["No available connections", "利用可能な接続がありません", "No hay conexiones disponibles"],
"选择允许查询的群或单聊": ["Choose allowed group or direct conversations", "照会を許可するグループ・個人会話を選択", "Selecciona conversaciones grupales o directas permitidas"],
"读取会话失败": ["Could not load conversations", "会話を読み込めませんでした", "No se pudieron cargar las conversaciones"],
"高级设置":["Advanced settings", "詳細設定", "Configuración avanzada"],
"状态字段":["State fields", "状態フィールド", "Campos de estado"],
"工具与资源":["Tools and resources", "ツールとリソース", "Herramientas y recursos"],
"编辑工具与资源":["Edit tools and resources", "ツールとリソースを編集", "Editar herramientas y recursos"],
"返回连接":["Back to connections", "接続に戻る", "Volver a conexiones"],
"放弃未保存的更改？":["Discard unsaved changes?", "未保存の変更を破棄しますか？", "¿Descartar los cambios sin guardar?"],
"返回后，本次修改不会保存。":["Your changes will not be saved.", "変更は保存されません。", "Los cambios no se guardarán."],
"放弃更改":["Discard changes", "変更を破棄", "Descartar cambios"],
"资源状态":["Resource state", "リソース状態", "Estado del recurso"],
"添加工具":["Add tool", "ツールを追加", "Añadir herramienta"],
"新工具":["New tool", "新しいツール", "Nueva herramienta"],
"工具标识":["Tool ID", "ツール ID", "ID de herramienta"],
"添加字段":["Add field", "フィールドを追加", "Añadir campo"],
"字段名称":["Field name", "フィールド名", "Nombre del campo"],
"值来源":["Value source", "値の取得元", "Origen del valor"],
"固定值":["Literal", "固定値", "Valor fijo"],
"调用 ID":["Invocation ID", "呼び出し ID", "ID de invocación"],
"参数路径":["Argument path", "引数パス", "Ruta del argumento"],
"自动生成":["Generated automatically", "自動生成", "Generado automáticamente"],
"完成条件":["Completion condition", "完了条件", "Condición de finalización"],
"Broker 已接受":["Broker accepted", "Broker が受理", "Aceptado por el broker"],
"设备回报确认":["Device confirmation", "デバイスからの確認", "Confirmación del dispositivo"],
"回执 Topic":["Reply topic", "応答 Topic", "Topic de respuesta"],
"关联 ID 路径":["Correlation ID path", "関連 ID パス", "Ruta del ID de correlación"],
"结果字段路径":["Result field path", "結果フィールドのパス", "Ruta del resultado"],
"期望值":["Expected value", "期待値", "Valor esperado"],
"超时（毫秒）":["Timeout (ms)", "タイムアウト（ミリ秒）", "Tiempo de espera (ms)"],
"删除工具":["Delete tool", "ツールを削除", "Eliminar herramienta"],
"填入智能开关示例":["Use smart switch example", "スマートスイッチの例を入力", "Usar ejemplo de interruptor"],
"添加资源":["Add resource", "リソースを追加", "Añadir recurso"],
"新资源":["New resource", "新しいリソース", "Nuevo recurso"],
"资源标识":["Resource ID", "リソース ID", "ID del recurso"],
"添加状态分组":["Add state group", "状態グループを追加", "Añadir grupo de estado"],
"删除资源":["Delete resource", "リソースを削除", "Eliminar recurso"],
"状态分组":["State group", "状態グループ", "Grupo de estado"],
"分组标识":["Group ID", "グループ ID", "ID del grupo"],
"上报 Topic":["Report topic", "レポート Topic", "Topic de informes"],
"有效期（毫秒，0 不过期）":["Max age (ms; 0 never expires)", "有効期間（ミリ秒、0 は無期限）", "Vigencia (ms; 0 sin caducidad)"],
"刷新工具":["Refresh tool", "更新ツール", "Herramienta de actualización"],
"设备时间路径（毫秒）":["Device timestamp path (ms)", "デバイス時刻パス（ミリ秒）", "Ruta de fecha del dispositivo (ms)"],
"整体替换分组；默认按字段合并":["Replace group; merge fields by default", "グループ全体を置換（既定はフィールドごとに結合）", "Reemplazar grupo; fusionar campos por defecto"],
"仅匹配指定报文":["Match selected reports only", "指定レポートのみ照合", "Filtrar informes"],
"匹配字段路径":["Match field path", "照合フィールドのパス", "Ruta del campo de filtro"],
"状态字段名称":["State field name", "状態フィールド名", "Nombre del estado"],
"上报 JSON 路径":["Report JSON path", "レポート JSON パス", "Ruta JSON del informe"],
"数据类型":["Data type", "データ型", "Tipo de dato"],
"等于时为真":["True when equal to", "一致した場合は true", "Verdadero si es igual a"],
"添加状态字段":["Add state field", "状態フィールドを追加", "Añadir campo de estado"],
"删除分组":["Delete group", "グループを削除", "Eliminar grupo"],
"读取状态失败":["Unable to read state", "状態を取得できません", "No se pudo leer el estado"],
"接收时间":["Received at", "受信日時", "Recibido"],
"有效":["Fresh", "有効", "Vigente"],
"已过期":["Stale", "期限切れ", "Caducado"],
"尚未配置资源":["No resources configured", "リソース未設定", "Sin recursos configurados"],
"JSON 参数定义":["JSON input schema", "JSON 入力スキーマ", "Esquema JSON de entrada"],
"JSON 值":["JSON value", "JSON 値", "Valor JSON"],
"请输入有效的 JSON 值":["Enter valid JSON", "有効な JSON を入力してください", "Introduce JSON válido"],
"标识必填且不能重复":["ID is required and must be unique", "ID は必須で重複できません", "El ID es obligatorio y único"],
"请填写说明":["Description is required", "説明を入力してください", "Introduce una descripción"],
"请填写名称":["Name is required", "名前を入力してください", "Introduce un nombre"],
"请填写有效的发布 Topic":["Enter an exact topic without wildcards", "ワイルドカードのない Topic を入力してください", "Introduce un topic sin comodines"],
"请输入有效的 JSON 对象":["Enter a valid JSON object", "有効な JSON オブジェクトを入力してください", "Introduce un objeto JSON válido"],
"至少添加一个消息字段":["Add at least one message field", "メッセージフィールドを追加してください", "Añade un campo de mensaje"],
"至少添加一个状态分组":["Add at least one state group", "状態グループを追加してください", "Añade un grupo de estado"],
"至少添加一个状态字段":["Add at least one state field", "状態フィールドを追加してください", "Añade un campo de estado"],
"请检查工具和资源状态配置。":["Check tool and resource configuration.", "ツールとリソースの設定を確認してください。", "Revisa la configuración de herramientas y recursos."],
"状态分组的 Topic 必须包含在订阅范围内。":["State topics must be covered by the subscription.", "状態 Topic は購読範囲に含めてください。", "Los topics de estado deben estar incluidos en la suscripción."],
"回复":["Response", "返信", "Respuesta"],
"回复内容":["Reply content", "返信内容", "Contenido de respuesta"],
"直接回复":["Direct reply", "直接返信", "Respuesta directa"],
"请检查能力、调用参数、用户授权和回复范围。": ["Check the capability, arguments, sender permissions and reply scope.", "機能、引数、ユーザー権限と返信範囲を確認してください。", "Comprueba la capacidad, los argumentos, los permisos y el ámbito de respuesta."],
"未开始的能力调用将停止，已完成的调用不会撤销。": ["Pending capability calls will stop. Completed calls cannot be undone.", "未実行の機能呼び出しは停止します。完了した操作は取り消されません。", "Las llamadas pendientes se detendrán. Las operaciones completadas no se desharán."],
"回复原会话": ["Reply to source conversation", "元の会話に返信", "Responder en la conversación original"],
"用户 OpenID，每行一个": ["User OpenID, one per line", "ユーザー OpenID（1 行に 1 件）", "OpenID de usuario, uno por línea"],
"允许触发的用户": ["Allowed senders", "実行を許可するユーザー", "Usuarios autorizados"],
"参数格式": ["Parameter schema", "パラメーター形式", "Esquema de parámetros"],
"选择连接": ["Select connection", "接続を選択", "Seleccionar conexión"],
"发送文本消息": ["Send text message", "テキストメッセージを送信", "Enviar mensaje de texto"],
"查询机器人信息": ["Get bot information", "ボット情報を取得", "Consultar información del bot"],
"调用能力": ["Invoke capability", "機能を呼び出す", "Invocar capacidad"],
"没有匹配的会话": ["No matching conversations", "一致する会話がありません", "No hay conversaciones coincidentes"],
"范围": ["Scope", "範囲", "Ámbito"],
"选择范围": ["Select scope", "範囲を選択", "Seleccionar ámbito"],
"全部消息": ["All messages", "すべてのメッセージ", "Todos los mensajes"],
"连接地址": ["Connection address", "接続先アドレス", "Dirección de conexión"],
"全部订阅消息": ["All subscribed messages", "すべての購読メッセージ", "Todos los mensajes suscritos"],
"指定 Topic": ["Specific topic", "Topic を指定", "Topic específico"],
"Topic 筛选": ["Topic filter", "Topic フィルター", "Filtro de topic"],
"默认发布 Topic": ["Default publish topic", "既定の送信 Topic", "Topic de publicación predeterminado"],
"指定群": ["Selected groups", "グループを指定", "Grupos seleccionados"],
"指定单聊": ["Selected direct chats", "個別チャットを指定", "Chats directos seleccionados"],
"所有群": ["All groups", "すべてのグループ", "Todos los grupos"],
"所有单聊": ["All direct chats", "すべての個別チャット", "Todos los chats directos"],
"全部会话": ["All conversations", "すべての会話", "Todas las conversaciones"],
"回复来源会话": ["Reply to source conversation", "元の会話へ返信", "Responder a la conversación de origen"],
"连接默认群": ["Connection default group", "接続の既定グループ", "Grupo predeterminado de la conexión"],
"群": ["Groups", "グループ", "Grupos"],
"单聊": ["Direct chats", "個別チャット", "Chats directos"],
"搜索会话": ["Search conversations", "会話を検索", "Buscar conversaciones"],
"搜索名称或 OpenID": ["Search name or OpenID", "名前または OpenID を検索", "Buscar nombre u OpenID"],
"搜索 OpenID": ["Search OpenID", "OpenID を検索", "Buscar OpenID"],
"无法读取会话": ["Unable to load conversations", "会話を読み込めません", "No se pueden cargar las conversaciones"],
"暂无会话，先向机器人发送一条消息。": ["No conversations yet. Send the bot a message first.", "会話がありません。まずボットにメッセージを送信してください。", "Aún no hay conversaciones. Envía un mensaje al bot."],
"限定群（可选）": ["Restrict group (optional)", "グループ制限（任意）", "Restringir grupo (opcional)"],
"留空，在路由中选择会话": ["Leave empty to select conversations in routes", "空欄の場合、ルートで会話を選択", "Dejar vacío para seleccionar conversaciones en rutas"],
"用户 OpenID，逗号分隔；留空允许所有人": ["User OpenIDs, comma-separated; empty allows everyone", "ユーザー OpenID をカンマ区切りで指定。空欄は全員を許可", "OpenIDs separados por comas; vacío permite a todos"],
"请选择接收会话范围。": ["Select the source conversation scope.", "受信する会話の範囲を選択してください。", "Selecciona el ámbito de conversaciones de origen."],
"请选择发送目标。": ["Select a destination.", "送信先を選択してください。", "Selecciona un destino."],
"回复来源会话需要使用同一个连接。": ["Reply to source requires the same connection.", "元の会話への返信には同じ接続が必要です。", "Responder al origen requiere la misma conexión."],
"Topic 筛选必须在连接订阅范围内。": ["Topic filter must stay within the subscription.", "Topic フィルターは購読範囲内で指定してください。", "El filtro debe estar dentro de la suscripción."],
"路由范围与连接不兼容，请重新选择。": ["Routing scope is incompatible with this connection.", "ルート範囲が接続と互換性がありません。選択し直してください。", "Ámbito incompatible con la conexión. Vuelve a seleccionarlo."],

"密钥环境变量名称无效：请以字母或下划线开头，仅使用字母、数字和下划线；不要填写密钥内容。": ["Invalid secret environment variable name: start with a letter or underscore and use only letters, digits and underscores. Do not enter the secret value.", "シークレット環境変数名が無効です。英字または下線で始め、英数字と下線のみ使用してください。値は入力しないでください。", "Nombre de variable no válido: empieza por letra o guion bajo y usa solo letras, números y guiones bajos. No introduzcas el secreto."],
"密钥环境变量的值为空或过长，请检查后端配置。": ["The secret environment variable is empty or too long. Check the backend configuration.", "シークレット環境変数の値が空か長すぎます。バックエンド設定を確認してください。", "El valor de la variable está vacío o es demasiado largo. Revisa la configuración del servidor."],
"密钥": ["Secret", "シークレット", "Secreto"],
"已配置，留空保持不变": ["Configured; leave blank to keep", "設定済み。空欄で変更なし", "Configurado; deja vacío para conservar"],
"填写 AppSecret": ["Enter AppSecret", "AppSecret を入力", "Introduce AppSecret"],
"请填写密钥；更换 AppID 后需要重新填写。": ["Enter the secret. A new AppID requires its secret.", "シークレットを入力してください。AppID 変更時は再入力が必要です。", "Introduce el secreto. Un AppID nuevo requiere su secreto."],
"AppID 必须为 1–32 位数字。": ["AppID must contain 1–32 digits.", "AppID は 1～32 桁の数字で入力してください。", "AppID debe tener entre 1 y 32 dígitos."],
"QoS 仅支持 0 或 1。": ["QoS must be 0 or 1.", "QoS は 0 または 1 のみです。", "QoS debe ser 0 o 1."],
"只填写 Broker 主机名或 IP，不包含协议、端口或路径。": ["Enter only the broker hostname or IP, without scheme, port or path.", "Broker のホスト名か IP のみ入力してください。プロトコル、ポート、パスは不要です。", "Introduce solo el host o IP del broker, sin protocolo, puerto ni ruta."],
"名称过长或含有控制字符。": ["The name is too long or contains control characters.", "名前が長すぎるか、制御文字が含まれています。", "El nombre es demasiado largo o contiene caracteres de control."],
"后端未配置此环境变量。": ["This environment variable is not configured on the backend.", "この環境変数はバックエンドに設定されていません。", "Esta variable no está configurada en el servidor."],
"填写完整的 HTTPS 地址；使用 HTTP 时需勾选允许普通 HTTP。": ["Enter a full HTTPS URL; enable plain HTTP to use an HTTP URL.", "完全な HTTPS URL を入力してください。HTTP の場合は通常の HTTP を許可してください。", "Introduce una URL HTTPS completa; habilita HTTP simple para usar HTTP."],
"客户端证书和私钥必须同时配置。": ["Configure the client certificate and private key together.", "クライアント証明書と秘密鍵を両方設定してください。", "Configura juntos el certificado de cliente y la clave privada."],
"客户端证书无效或与私钥不匹配。": ["The client certificate is invalid or does not match the private key.", "クライアント証明書が無効か、秘密鍵と一致しません。", "El certificado no es válido o no coincide con la clave privada."],
"密钥不能包含空格、换行或非 ASCII 字符。": ["The secret cannot contain spaces, line breaks or non-ASCII characters.", "シークレットに空白、改行、非 ASCII 文字は使用できません。", "El secreto no puede contener espacios, saltos de línea ni caracteres no ASCII."],
"当前后端不支持此操作，请检查服务版本。": ["This backend does not support the operation. Check its version.", "このバックエンドは操作に未対応です。バージョンを確認してください。", "El servidor no admite esta operación. Comprueba su versión."],
"当前操作没有权限。": ["You do not have permission for this operation.", "この操作を実行する権限がありません。", "No tienes permiso para esta operación."],
"当前页面地址未被后端允许，请使用配置的管理台地址或配置允许的来源。": ["This page origin is not allowed. Use the configured console address or configure the allowed origins.", "このページのオリジンは許可されていません。設定済みの管理画面 URL を使うか、許可するオリジンを設定してください。", "El origen de esta página no está permitido. Usa la dirección configurada o configura los orígenes permitidos."],
"成员 OpenID 格式不正确或超过 100 个。": ["Member OpenIDs are invalid or exceed the limit of 100.", "メンバー OpenID が無効か、100 件を超えています。", "Los OpenID no son válidos o superan el límite de 100."],
"提交内容过大，请缩短配置内容。": ["The request is too large. Reduce the configuration size.", "送信内容が大きすぎます。設定内容を短くしてください。", "La solicitud es demasiado grande. Reduce la configuración."],
"最多填写 100 个有效的成员 OpenID，使用英文逗号分隔。": ["Enter up to 100 valid member OpenIDs, separated by commas.", "有効なメンバー OpenID を半角カンマ区切りで最大 100 件入力してください。", "Introduce hasta 100 OpenID válidos, separados por comas."],
"此环境变量中的 CA 证书无效。": ["The CA certificate in this variable is invalid.", "この環境変数の CA 証明書が無効です。", "El certificado CA de esta variable no es válido."],
"此环境变量中的密码无效。": ["The password in this variable is invalid.", "この環境変数のパスワードが無効です。", "La contraseña de esta variable no es válida."],
"此环境变量中的私钥无效。": ["The private key in this variable is invalid.", "この環境変数の秘密鍵が無効です。", "La clave privada de esta variable no es válida."],
"此环境变量的值为空或过长。": ["This variable is empty or too long.", "この環境変数の値が空か、長すぎます。", "Esta variable está vacía o es demasiado larga."],
"此认证信息无法构成有效的 HTTP 请求头。": ["These credentials cannot form a valid HTTP header.", "この認証情報では有効な HTTP ヘッダーを作成できません。", "Estas credenciales no forman una cabecera HTTP válida."],
"此连接已不存在，请刷新列表。": ["This connection no longer exists. Refresh the list.", "この接続は既にありません。一覧を更新してください。", "Esta conexión ya no existe. Actualiza la lista."],
"用户名不能为空或含有空字符，最多 256 字节。": ["Username must be nonempty, at most 256 bytes, and contain no null characters.", "ユーザー名は空文字やヌル文字を含まない 256 バイト以内にしてください。", "El usuario no puede estar vacío ni contener caracteres nulos; máximo 256 bytes."],
"用户名不能包含冒号，最多 256 字节。": ["Username cannot contain a colon and must be at most 256 bytes.", "ユーザー名はコロンを含まない 256 バイト以内にしてください。", "El usuario no puede contener dos puntos; máximo 256 bytes."],
"端口必须为 1–65535 的整数。": ["Port must be an integer from 1 to 65535.", "ポートは 1～65535 の整数で入力してください。", "El puerto debe ser un entero entre 1 y 65535."],
"群 OpenID 只能包含字母、数字、下划线和短横线，最多 128 字符。": ["Group OpenID allows letters, digits, underscores and hyphens, up to 128 characters.", "グループ OpenID は英数字、下線、ハイフンのみ、最大 128 文字です。", "OpenID admite letras, números, guiones y guiones bajos; máximo 128 caracteres."],
"群 OpenID 格式不正确。": ["Group OpenID has an invalid format.", "グループ OpenID の形式が不正です。", "El formato del OpenID del grupo no es válido."],
"至少启用订阅接收或发布发送。": ["Enable subscribing, publishing, or both.", "購読受信または発行送信を有効にしてください。", "Activa la suscripción, la publicación o ambas."],
"说明不能超过 4000 字节。": ["Description cannot exceed 4000 bytes.", "説明は 4000 バイト以内にしてください。", "La descripción no puede superar los 4000 bytes."],
"请填写事件类型，不能包含控制字符，最多 128 字节。": ["Enter an event type without control characters, up to 128 bytes.", "制御文字を含まないイベントタイプを 128 バイト以内で入力してください。", "Introduce un tipo de evento sin caracteres de control, hasta 128 bytes."],
"请填写发布 Topic，不能包含 + 或 # 通配符。": ["Enter a publish topic without + or # wildcards.", "発行 Topic を入力してください。+ と # は使用できません。", "Introduce un tema de publicación sin comodines + ni #."],
"请填写名称。": ["Enter a name.", "名前を入力してください。", "Introduce un nombre."],
"请填写有效的环境变量名称，以字母或下划线开头。": ["Enter a valid environment variable name starting with a letter or underscore.", "英字または下線で始まる有効な環境変数名を入力してください。", "Introduce un nombre de variable válido que empiece por letra o guion bajo."],
"请填写此 AppID 对应的 AppSecret。": ["Enter the AppSecret for this AppID.", "この AppID に対応する AppSecret を入力してください。", "Introduce el AppSecret correspondiente a este AppID."],
"请填写用户名，不能包含冒号，最多 256 字节。": ["Enter a username without colons, up to 256 bytes.", "コロンを含まないユーザー名を 256 バイト以内で入力してください。", "Introduce un usuario sin dos puntos, hasta 256 bytes."],
"请填写订阅 Topic；+ 占一个层级，# 只能位于最后一个层级。": ["Enter a subscription topic; + occupies one level and # must be the final level.", "購読 Topic を入力してください。+ は 1 階層、# は最後の階層のみ使用できます。", "Introduce un tema de suscripción; + ocupa un nivel y # debe ser el último nivel."],
"请检查发布 Topic 和 QoS。": ["Check the publish topic and QoS.", "発行 Topic と QoS を確認してください。", "Comprueba el tema de publicación y QoS."],
"请检查订阅 Topic、QoS 和事件类型。": ["Check the subscription topic, QoS and event type.", "購読 Topic、QoS、イベントタイプを確認してください。", "Comprueba el tema de suscripción, QoS y tipo de evento."],
"请求头需以 x- 开头，仅含小写字母、数字和短横线，不能使用签名专用头。": ["Use a header starting with x- containing lowercase letters, digits and hyphens; signature headers are reserved.", "ヘッダーは x- で始まる小文字英数字とハイフンのみです。署名用ヘッダーは使用できません。", "La cabecera debe empezar por x- y usar minúsculas, números y guiones; las cabeceras de firma están reservadas."],
"请求格式不兼容，请刷新页面并确认前后端版本一致。": ["Incompatible request format. Refresh and check that frontend and backend versions match.", "リクエスト形式が非互換です。更新し、フロントエンドとバックエンドのバージョンを確認してください。", "Formato incompatible. Actualiza y comprueba que las versiones de interfaz y servidor coincidan."],
"请求过于频繁，请稍后重试。": ["Too many requests. Try again later.", "リクエストが多すぎます。後でもう一度お試しください。", "Demasiadas solicitudes. Inténtalo más tarde."],
"请选择 POST、PUT 或 PATCH。": ["Select POST, PUT or PATCH.", "POST、PUT、PATCH のいずれかを選択してください。", "Selecciona POST, PUT o PATCH."],
"连接编号无效，请重新打开表单。": ["Invalid connection ID. Reopen the form.", "接続 ID が無効です。フォームを開き直してください。", "ID de conexión no válido. Vuelve a abrir el formulario."],
"配置密码时需要填写用户名。": ["A username is required when setting a password.", "パスワードを設定する場合はユーザー名が必要です。", "Se requiere usuario al configurar una contraseña."],
"请检查标红的输入项。": ["Check the highlighted fields.", "赤く表示された項目を確認してください。", "Comprueba los campos resaltados."],
"保存失败，请稍后重试。": ["Save failed. Try again later.", "保存に失敗しました。後でもう一度お試しください。", "No se pudo guardar. Inténtalo más tarde."],
"此 AppID 已有 QQ 连接，请编辑已有连接。": ["This AppID already has a QQ connection. Edit that connection.", "この AppID の QQ 接続は既に存在します。既存の接続を編集してください。", "Este AppID ya tiene una conexión QQ. Edita la conexión existente."],
"群 OpenID": ["Group OpenID", "グループ OpenID", "OpenID del grupo"],
"填写群 OpenID，不是 QQ 群号": ["Group OpenID, not the numeric QQ group number", "QQ グループ番号ではなく OpenID", "OpenID del grupo, no su número de QQ"],
"允许的成员": ["Allowed members", "許可するメンバー", "Miembros permitidos"],
"成员 OpenID，逗号分隔；留空允许群内所有人": ["Member OpenIDs, comma-separated; empty allows all group members", "メンバー OpenID をカンマ区切りで入力。空欄は全員を許可", "OpenIDs separados por comas; vacío permite a todos los miembros"],
"接收方式": ["Receive transport", "受信方式", "Transporte de recepción"],
"允许主动发送": ["Allow proactive messages", "能動的な送信を許可", "Permitir mensajes proactivos"],
"请检查 AppID、群 OpenID 和成员 OpenID。": ["Check AppID, group OpenID and member OpenIDs.", "AppID、グループとメンバーの OpenID を確認してください。", "Comprueba AppID y los OpenID del grupo y miembros."],
"QQ 密钥无效，请重新填写 AppSecret。": ["Invalid QQ secret. Enter AppSecret again.", "QQ シークレットが無効です。AppSecret を再入力してください。", "Secreto QQ no válido. Vuelve a introducir AppSecret."],
"添加消息连接，再到路由中选择来源和目标。": ["Add message connections, then choose sources and destinations in Routes.", "メッセージ接続を追加し、ルートで送信元と送信先を選択します。", "Añade conexiones y elige orígenes y destinos en Rutas."],
"按指令映射生成消息。": ["Generate the message using the command mapping.", "コマンドマッピングでメッセージを生成します。", "Genera el mensaje con el mapeo del comando."],
"Agent（HTTP 接收端）": ["Agent (HTTP receiver)", "Agent（HTTP 受信）", "Agent (receptor HTTP)"],
"发送到 Agent 的 HTTP 接收端，结果通过回调入口返回。": ["Send to an Agent HTTP receiver; results return through a callback connection.", "Agent の HTTP エンドポイントへ送信し、結果はコールバック接続で受信します。", "Envía al receptor HTTP del Agent; los resultados vuelven mediante una conexión de callback."],
"请填写 Agent 名称并选择 HTTP 目标。": ["Enter an Agent name and select HTTP destinations.", "Agent 名と HTTP 送信先を指定してください。", "Introduce el nombre del Agent y elige destinos HTTP."],
"所有命中的路由都会执行。": ["All matching routes are executed.", "一致したすべてのルートを実行します。", "Se ejecutan todas las rutas coincidentes."],
"指令映射": ["Command mapping", "コマンドマッピング", "Mapeo de comandos"],
"匹配方式": ["Match mode", "一致方法", "Modo de coincidencia"],
"全文匹配": ["Exact text", "完全一致", "Texto exacto"],
"指令与参数": ["Command and arguments", "コマンドと引数", "Comando y argumentos"],
"指令内容": ["Command text", "コマンド内容", "Texto del comando"],
"消息字段": ["Message field", "メッセージのフィールド", "Campo del mensaje"],
"JSON Pointer，例如 /content 或 /text；留空读取 JSON 字符串。": ["JSON Pointer, e.g. /content or /text; leave empty for a JSON string.", "JSON Pointer（/content、/text など）。JSON 文字列の場合は空欄。", "JSON Pointer, p. ej. /content o /text; vacío para una cadena JSON."],
"映射为 JSON": ["Map to JSON", "JSON に変換", "Convertir a JSON"],
"输出事件类型": ["Output event type", "出力イベント種別", "Tipo de evento de salida"],
"消息模板": ["Message template", "メッセージテンプレート", "Plantilla de mensaje"],
"请输入有效的 JSON。": ["Enter valid JSON.", "有効な JSON を入力してください。", "Introduce JSON válido."],
"参数使用 {\"$command\":\"arguments\"}，原消息字段使用 {\"$payload\":\"/字段\"}。": ["Arguments: {\"$command\":\"arguments\"}; payload fields: {\"$payload\":\"/field\"}.", "引数: {\"$command\":\"arguments\"}、元の値: {\"$payload\":\"/field\"}。", "Argumentos: {\"$command\":\"arguments\"}; campos: {\"$payload\":\"/field\"}."],
"指令映射无效，请检查匹配内容、消息字段和模板。": ["Invalid command mapping. Check the trigger, message field and template.", "コマンド、フィールド、テンプレートを確認してください。", "Mapeo no válido. Revisa el comando, campo y plantilla."],
"附带 Topic 信息": ["Include topic metadata", "トピック情報を含める", "Incluir metadatos del tema"],
"消息连接": ["Message connections", "メッセージ接続", "Conexiones de mensajes"],
"HTTP 工具": ["HTTP tools", "HTTP ツール", "Herramientas HTTP"],
"添加 HTTP 工具": ["Add HTTP tool", "HTTP ツールを追加", "Añadir herramienta HTTP"],
"尚未添加 HTTP 工具": ["No HTTP tools yet", "HTTP ツールはありません", "Aún no hay herramientas HTTP"],
"发送内容": ["Message body", "送信内容", "Contenido enviado"],
"原始内容": ["Original payload", "元のペイロード", "Contenido original"],
"完整事件": ["Full event", "イベント全体", "Evento completo"],
"密钥环境变量": ["Secret environment variable", "シークレット環境変数", "Variable de entorno del secreto"],
"接收格式": ["Input format", "受信形式", "Formato de entrada"],
"事件编号请求头": ["Event ID header", "イベント ID ヘッダー", "Cabecera del ID de evento"],
"请求方式": ["HTTP method", "HTTP メソッド", "Método HTTP"],
"允许普通 HTTP": ["Allow plain HTTP", "平文 HTTP を許可", "Permitir HTTP sin cifrar"],
"端口": ["Port", "ポート", "Puerto"],
"CA 环境变量": ["CA environment variable", "CA 環境変数", "Variable de entorno de CA"],
"留空使用公共 CA": ["Leave empty for public CAs", "空欄で公開 CA を使用", "Vacío para CA públicas"],
"客户端证书环境变量": ["Client certificate variable", "クライアント証明書の環境変数", "Variable del certificado de cliente"],
"客户端私钥环境变量": ["Client private key variable", "クライアント秘密鍵の環境変数", "Variable de la clave privada"],
"订阅接收": ["Subscribe / receive", "購読・受信", "Suscribir / recibir"],
"发布发送": ["Publish / send", "配信・送信", "Publicar / enviar"],
"订阅 Topic": ["Subscription topic", "購読トピック", "Tema de suscripción"],
"发布 Topic": ["Publish topic", "配信トピック", "Tema de publicación"],
"连接仍有待处理消息，请处理后再修改或删除。": ["This connection has unresolved messages. Resolve them before changing its configuration or deleting it.", "未処理のメッセージがあります。設定変更や削除の前に処理してください。", "Hay mensajes pendientes. Resuélvelos antes de cambiar la configuración o eliminarla."],
"发布 Topic 不能匹配同一连接的订阅 Topic。": ["The publish topic must not match this connection’s subscription.", "配信トピックは同じ接続の購読に一致させないでください。", "El tema de publicación no debe coincidir con la suscripción de esta conexión."],
"找不到环境变量，请先在后端配置。": ["Environment variable missing. Configure it on the backend first.", "環境変数がありません。バックエンドで設定してください。", "Falta la variable de entorno. Configúrala en el servidor."],
"Webhook 密钥须为 32–4096 个可见 ASCII 字符。": ["The Webhook secret must contain 32–4096 visible ASCII characters.", "Webhook シークレットには 32～4096 の可視 ASCII 文字が必要です。", "El secreto Webhook debe tener entre 32 y 4096 caracteres ASCII visibles."],
"配置已更新，请刷新后重试。": ["Configuration changed. Refresh and retry.", "設定が更新されました。再読み込みして再試行してください。", "La configuración cambió. Actualiza y reintenta."],
"读取连接失败，请刷新重试。": ["Could not load connections. Refresh to retry.", "接続を読み込めません。再読み込みしてください。", "No se pudieron cargar las conexiones. Actualiza para reintentar."],
"配置无效，请检查地址、Topic、证书和环境变量。": ["Invalid configuration. Check the address, topic, certificates and environment variables.", "設定が無効です。アドレス、トピック、証明書、環境変数を確認してください。", "Configuración no válida. Revisa dirección, tema, certificados y variables de entorno."],
"消息连接接口尚不可用，请重启新版后端。": ["Message connections are unavailable. Restart the updated backend.", "メッセージ接続 API を利用できません。更新したバックエンドを再起動してください。", "Las conexiones de mensajes no están disponibles. Reinicia el servidor actualizado."],
"添加 MQTT、Webhook 或 HTTP Client，再到路由中选择来源和目标。": ["Add MQTT, Webhook or HTTP Client connections, then choose sources and destinations in Routes.", "MQTT、Webhook、HTTP Client を追加し、ルートで送信元と送信先を選択します。", "Añade conexiones MQTT, Webhook o HTTP Client y selecciona origen y destino en Rutas."],
"同时移除路由中的引用；没有目标的路由一并删除。历史消息保留。": ["Also removes route references and routes with no remaining destinations. Message history is retained.", "ルートの参照と送信先がなくなるルートも削除します。メッセージ履歴は保持されます。", "También elimina referencias y rutas sin destinos restantes. Se conserva el historial."],
"连接类型": ["Connection type", "接続の種類", "Tipo de conexión"],
"接收地址": ["Receiver URL", "受信 URL", "URL de recepción"],

"应用消息": ["App message", "アプリメッセージ", "Mensaje de la aplicación"],
"夜间设备告警": ["Nighttime device alert", "夜間デバイス警告", "Alerta nocturna del dispositivo"],
"设备告警": ["Device alert", "デバイス警告", "Alerta del dispositivo"],
"设备状态": ["Device status", "デバイス状態", "Estado del dispositivo"],
"设备指令": ["Device command", "デバイス指令", "Comando del dispositivo"],
"第 {0} / {1} 页": ["Page {0} of {1}", "{0} / {1} ページ", "Página {0} de {1}"],
"下一页": ["Next", "次へ", "Siguiente"],
"上一页": ["Previous", "前へ", "Anterior"],
"共 {0} 条": ["{0} routes", "全 {0} 件", "{0} rutas"],
"路由分页": ["Route pagination", "ルートのページ切り替え", "Paginación de rutas"],
"没有符合条件的路由": ["No matching routes", "一致するルートはありません", "No hay rutas coincidentes"],
"入口筛选（多选）": ["Filter sources (multiple selection)", "入力元フィルター（複数選択）", "Filtrar entradas (selección múltiple)"],
"入口筛选（单选）": ["Filter sources (single selection)", "入力元フィルター（単一選択）", "Filtrar entradas (selección única)"],
"全部": ["All", "すべて", "Todos"],
"待接入": ["Awaiting connection", "接続待ち", "Pendiente de conexión"],
"Agent 执行器未接入": ["Agent executor is not connected", "Agent 実行環境が未接続です", "El ejecutor del agente no está conectado"],
"Agent 处理（待接入）": ["Agent processing (not connected)", "Agent 処理（接続待ち）", "Procesamiento por agente (sin conectar)"],
"Agent 名称": ["Agent name", "Agent 名", "Nombre del agente"],
"仅保存处理流程，接入执行器后才能启用。": ["Saves the flow only. An executor must be connected before it can run.", "フローのみ保存します。有効化には実行環境の接続が必要です。", "Solo guarda el flujo. Conecta un ejecutor para poder activarlo."],
"路由": ["Routes", "ルート", "Rutas"],
"路由视图": ["Route view", "ルート表示", "Vista de rutas"],
"路径列表": ["Paths", "経路一覧", "Rutas"],
"关系图": ["Connections graph", "関係図", "Mapa de conexiones"],
"新建路由": ["New route", "ルートを作成", "Nueva ruta"],
"编辑路由": ["Edit route", "ルートを編集", "Editar ruta"],
"删除路由": ["Delete route", "ルートを削除", "Eliminar ruta"],
"停止路由": ["Stop route", "ルートを停止", "Detener ruta"],
"正在读取路由…": ["Loading routes…", "ルートを読み込み中…", "Cargando rutas…"],
"路由暂不可用": ["Routes unavailable", "ルートを利用できません", "Rutas no disponibles"],
"当前为 Agent 处理路径": ["Agent processing is configured", "Agent 処理が設定されています", "Procesamiento con agente configurado"],
"尚未配置消息路径": ["No message pipeline configured", "メッセージ経路が未設定です", "No hay un flujo de mensajes configurado"],
"此路径由部署配置管理，暂不支持在线修改。": ["This path is managed by deployment configuration; online editing is not available yet.", "この経路はデプロイ設定で管理されており、オンライン編集にはまだ対応していません。", "Esta ruta se gestiona en la configuración del despliegue; aún no admite edición en línea."],
"先在部署配置中连接 Webhook 与 HTTP 发送端，再管理它们之间的路由。": ["Configure a Webhook receiver and HTTP destination in the deployment, then manage their routes here.", "デプロイ設定で Webhook 受信端と HTTP 送信先を接続すると、ここでルートを管理できます。", "Configura un receptor Webhook y un destino HTTP en el despliegue para gestionar sus rutas aquí."],
"尚未添加路由": ["No routes yet", "ルートはまだありません", "Aún no hay rutas"],
"选择来源和目标，建立第一条消息路径。": ["Choose a source and destination to create your first path.", "送信元と送信先を選び、最初の経路を作成します。", "Selecciona un origen y un destino para crear la primera ruta."],
"已提交 {0}": ["Submitted {0}", "送信済み {0}", "Enviados {0}"],
"待发送 {0}": ["Pending {0}", "送信待ち {0}", "Pendientes {0}"],
"需处理 {0}": ["Needs attention {0}", "要確認 {0}", "Requieren atención {0}"],
"统计为当前目标在保留期内的投递记录；已提交表示发送端接受，不代表设备已执行。": ["Counts cover retained deliveries for current targets. Submitted means accepted by the endpoint, not executed by a device.", "統計は現在の送信先の保持中の配信記録です。送信済みは受信端の受理を示し、機器の実行完了を意味しません。", "Los recuentos incluyen entregas conservadas de los destinos actuales. Enviado indica aceptación por el destino, no ejecución por el dispositivo."],
"处理": ["Processing", "処理", "Procesamiento"],
"目标": ["Destinations", "送信先", "Destinos"],
"保留事件类型、标识和原始内容。": ["Preserve event type, identifiers and original content.", "イベント種別、識別子、元の内容を保持します。", "Conserva el tipo de evento, identificadores y contenido original."],
"筛选条件": ["Filter", "フィルター", "Filtro"],
"事件类型": ["Event type", "イベント種別", "Tipo de evento"],
"留空表示全部": ["Leave empty for all events", "空欄ですべてのイベント", "Vacío para todos los eventos"],
"新路由保存为停止状态，启用后开始转发。": ["New routes are saved stopped. Enable to start forwarding.", "新しいルートは停止状態で保存されます。有効化すると転送を開始します。", "Las nuevas rutas se guardan detenidas. Actívalas para iniciar el reenvío."],
"仅影响尚未分发的消息，已进入发送队列的消息继续处理。恢复启用不会重放已处理的消息。": ["Only messages not yet dispatched are affected. Queued deliveries continue. Enabling does not replay processed messages.", "未分配のメッセージのみが対象です。送信キュー内の配信は継続します。再有効化しても処理済みメッセージは再送しません。", "Solo afecta a mensajes sin distribuir. Los envíos en cola continúan. Reactivar no reproduce mensajes procesados."],
"尚未确认提交，可使用原编号重试。": ["Commit not confirmed. Retry with the original operation ID.", "確定が未確認です。元の操作 ID で再試行できます。", "Confirmación pendiente. Reintenta con el identificador original."],
"接收": ["Receive", "受信", "Recibir"],
"选择目标": ["Select destinations", "送信先を選択", "Seleccionar destinos"],
"该名称被 OAuth 授权引用，请先调整服务端授权配置再重命名。": ["This name is referenced by OAuth grants. Update server grants before renaming.", "この名前は OAuth 権限で参照されています。権限設定を更新してから名前を変更してください。", "El nombre está referenciado por permisos OAuth. Actualiza los permisos antes de renombrarlo."],
"同时移除所有 MCP 服务及未发布配置中的引用。MCP 服务和历史记录保留。": ["References in all MCP servers and unpublished configurations will also be removed. MCP servers and history are retained.", "すべての MCP サービスと未公開設定の参照も削除します。サービスと履歴は保持されます。", "También se eliminarán las referencias de todos los servidores MCP y configuraciones sin publicar. Los servidores y el historial se conservan."],
"先查询原操作，确认结果后再继续。": ["Check the original operation before continuing.", "元の操作結果を確認してから続行してください。", "Consulta la operación original antes de continuar."],
"操作结果待确认": ["Operation awaiting confirmation", "操作結果を確認中", "Resultado pendiente de confirmar"],
"操作已完成。": ["Operation completed.", "操作が完了しました。", "Operación completada."],
"连接已删除，引用已清理。": ["Connection deleted and references removed.", "接続と参照を削除しました。", "Conexión eliminada y referencias retiradas."],
"删除连接": ["Delete connection", "接続を削除", "Eliminar conexión"],
"编辑连接": ["Edit connection", "接続を編集", "Editar conexión"],
"发送": ["Send", "送信", "Enviar"],
"请输入有效的 JSON Schema。": ["Enter a valid JSON Schema.", "有効な JSON Schema を入力してください。", "Introduce un JSON Schema válido."],
"输出参数": ["Output parameters", "出力パラメーター", "Parámetros de salida"],
"输入参数": ["Input parameters", "入力パラメーター", "Parámetros de entrada"],
"查看详情": ["View details", "詳細を表示", "Ver detalles"],
"连接详情": ["Connection details", "接続の詳細", "Detalles de conexión"],
"尚未添加连接": ["No connections yet", "接続はまだありません", "Aún no hay conexiones"],
"连接已保存。": ["Connection saved.", "接続を保存しました。", "Conexión guardada."],
"{0} 个连接": ["{0} connections", "{0} 件の接続", "{0} conexiones"],
"客户端 {0}": ["Clients {0}", "クライアント {0}", "Clientes {0}"],
"当前为无状态 HTTP，不保留客户端会话。": ["Stateless HTTP does not retain client sessions.", "ステートレス HTTP はクライアントセッションを保持しません。", "HTTP sin estado no mantiene sesiones de clientes."],
"更多工具 {0}": ["{0} more tools", "その他のツール {0}", "{0} herramientas más"],
"工具": ["Tools", "ツール", "Herramientas"],
"资源": ["Resources", "リソース", "Recursos"],
"提示词": ["Prompts", "プロンプト", "Prompts"],
"模板": ["Template", "テンプレート", "Plantilla"],
"参数": ["Parameters", "パラメーター", "Parámetros"],
"资源 {0}": ["Resources {0}", "リソース {0}", "Recursos {0}"],
"提示词 {0}": ["Prompts {0}", "プロンプト {0}", "Prompts {0}"],
"暂无工具": ["No tools available", "ツールがありません", "No hay herramientas"],
"暂无资源": ["No resources available", "リソースがありません", "No hay recursos"],
"暂无提示词": ["No prompts available", "プロンプトがありません", "No hay prompts"],
"内容目录暂不可用": ["Content catalog unavailable", "コンテンツ一覧を利用できません", "Catálogo de contenido no disponible"],
"开放资源": ["Exposed resources", "公開リソース", "Recursos publicados"],
"开放提示词": ["Exposed prompts", "公開プロンプト", "Prompts publicados"],
"尚未开放资源": ["No resources exposed", "公開リソースがありません", "No hay recursos publicados"],
"尚未开放提示词": ["No prompts exposed", "公開プロンプトがありません", "No hay prompts publicados"],
"资源或提示词不存在或重复，请刷新后重试。": ["Resources or prompts are missing or duplicated. Refresh and retry.", "リソースまたはプロンプトが存在しないか重複しています。再読み込みしてください。", "Los recursos o prompts no existen o están duplicados. Actualiza y reintenta."],
"选择认证方式": ["Select authentication", "認証方式を選択", "Seleccionar autenticación"],
"密钥环境变量不存在或格式无效，请检查服务端配置。": ["The secret environment variable is missing or invalid. Check the server configuration.", "秘密値の環境変数が存在しないか無効です。サーバー設定を確認してください。", "La variable de entorno del secreto falta o no es válida. Revisa la configuración del servidor."],
"认证配置无效，请检查公钥、签发者和地址等字段。": ["Invalid authentication settings. Check the public key, issuer and URLs.", "認証設定が無効です。公開鍵、発行者、URL を確認してください。", "Configuración de autenticación inválida. Revisa la clave pública, el emisor y las URL."],
"完整参数定义": ["Full parameter schema", "完全なパラメーター定義", "Esquema completo de parámetros"],
"无固定参数": ["No fixed parameters", "固定パラメーターなし", "Sin parámetros fijos"],
"必填": ["Required", "必須", "Obligatorio"],
"工具参数": ["Parameters", "パラメーター", "Parámetros"],
"说明": ["Description", "説明", "Descripción"],
"名称": ["Name", "名前", "Nombre"],
"认证与开放范围按服务独立管理，保存后对新请求生效。": ["Authentication and exposure are configured per server. Changes apply to new requests after saving.", "認証と公開範囲はサービスごとに設定します。保存後、新しいリクエストに適用されます。", "La autenticación y el alcance son independientes por servicio. Los cambios se aplican a nuevas solicitudes."],
"当前支持 Streamable HTTP；stdio 和旧版 SSE 尚未实现。": ["Streamable HTTP is available. stdio and legacy SSE are not implemented yet.", "現在は Streamable HTTP に対応しています。stdio と旧 SSE は未実装です。", "Disponible Streamable HTTP; stdio y SSE antiguo aún no están implementados."],
"允许本机 HTTP 授权服务": ["Allow local HTTP authorization server", "ローカル HTTP 認可サーバーを許可", "Permitir servidor de autorización HTTP local"],
"MCP 地址无效：请使用 HTTPS（本机可用 HTTP），路径为 /mcp 或 /mcp/名称，不含查询参数。": ["Invalid MCP URL: use HTTPS (HTTP on loopback), with /mcp or /mcp/name and no query parameters.", "MCP URL が無効です。HTTPS（ローカルは HTTP 可）、/mcp または /mcp/name、クエリなしを指定してください。", "URL MCP no válida: usa HTTPS (HTTP local), /mcp o /mcp/nombre, sin parámetros."],
"MCP 地址路径已被其他服务使用。": ["This MCP URL path is already used by another service.", "この MCP パスは別のサービスで使用されています。", "Otro servicio ya usa esta ruta MCP."],
"密钥填写服务端环境变量名称，不填写明文；保存时验证引用是否可用。": ["Enter a server environment variable name, not a secret. References are validated when saving.", "秘密値ではなくサーバーの環境変数名を入力してください。保存時に参照を検証します。", "Introduce el nombre de una variable del servidor, no el secreto. Se valida al guardar."],
"允许的 OAuth Client ID": ["Allowed OAuth client ID", "許可する OAuth Client ID", "OAuth Client ID permitido"],
"所需 Scope": ["Required scope", "必要な Scope", "Scope requerido"],
"客户端密钥环境变量": ["Client secret environment variable", "クライアントシークレットの環境変数", "Variable de entorno del secreto de cliente"],
"验证端点 Client ID": ["Introspection client ID", "検証用 Client ID", "Client ID de introspección"],
"令牌验证端点": ["Token introspection endpoint", "トークン検証エンドポイント", "Endpoint de introspección"],
"公开 MCP 地址": ["Public MCP URL", "公開 MCP URL", "URL pública MCP"],
"RSA 公钥 PEM": ["RSA public key (PEM)", "RSA 公開鍵 PEM", "Clave pública RSA (PEM)"],
"允许的主体 Subject": ["Allowed subject", "許可する主体 Subject", "Sujeto permitido"],
"目标资源 Audience": ["Audience", "対象 Audience", "Destinatario Audience"],
"签发者 Issuer": ["Issuer", "発行者 Issuer", "Emisor Issuer"],
"密码环境变量": ["Password environment variable", "パスワードの環境変数", "Variable de entorno de contraseña"],
"用户名": ["Username", "ユーザー名", "Usuario"],
"任何能够访问此地址的客户端都可匿名调用本服务的开放能力。": ["Any client that can reach this URL can anonymously use this server’s exposed capabilities.", "この URL にアクセスできるクライアントは、公開機能を匿名で利用できます。", "Cualquier cliente con acceso a esta URL puede usar anónimamente las capacidades publicadas."],
"保留当前部署的令牌或 OAuth 认证配置。": ["Keep the deployment token or OAuth configuration.", "現在のデプロイのトークンまたは OAuth 設定を維持します。", "Conserva el token o la configuración OAuth del despliegue."],
"无鉴权": ["No authentication", "認証なし", "Sin autenticación"],
"沿用部署认证": ["Use deployment authentication", "デプロイの認証を使用", "Usar autenticación del despliegue"],
"管理工具": ["Manage tools", "ツールを管理", "Gestionar herramientas"],
"当前为无状态 HTTP，暂未提供客户端会话统计。": ["Stateless HTTP is in use; client session statistics are not available yet.", "ステートレス HTTP のため、クライアントのセッション統計はまだ提供されていません。", "HTTP sin estado; aún no hay estadísticas de sesiones de clientes."],
"客户端 —": ["Clients —", "クライアント —", "Clientes —"],
"停止": ["Stop", "停止", "Detener"],
"说明这个服务的用途，例如：查询和控制家里的智能设备": ["Describe its purpose, e.g. check and control home devices", "例：家庭のスマートデバイスの状態確認と操作", "Describe su uso, por ejemplo: consultar y controlar dispositivos del hogar"],
"服务说明": ["Description", "サービスの説明", "Descripción"],
"认证配置由当前部署统一管理，所有 MCP 服务共用。": ["Authentication is managed for this deployment and shared by all MCP servers.", "認証設定はデプロイ単位で管理され、すべての MCP サービスで共有されます。", "La autenticación se gestiona por despliegue y se comparte entre todos los servicios MCP."],
"管理开放范围": ["Manage access scope", "公開範囲を管理", "Gestionar alcance"],
"此处仅列出已开放工具；资源和提示词不包含在这个数量中。": ["Only published tools are listed here. Resources and prompts are not included in this count.", "ここには公開ツールのみを表示します。リソースとプロンプトはこの件数に含まれません。", "Solo se muestran herramientas publicadas. Los recursos y prompts no se incluyen en este número."],
"尚未开放工具": ["No tools published yet", "公開ツールはありません", "Aún no hay herramientas publicadas"],
"暂无说明": ["No description", "説明なし", "Sin descripción"],
"这是本机地址。云端 Agent 接入需要可访问的 HTTPS 地址。": ["This is a local address. Cloud agents need a reachable HTTPS URL.", "これはローカル URL です。クラウド Agent にはアクセス可能な HTTPS URL が必要です。", "Esta dirección es local. Los agentes en la nube necesitan una URL HTTPS accesible."],
"示例中的 YOUR_MCP_TOKEN 需要在客户端替换为实际令牌。": ["Replace YOUR_MCP_TOKEN with your actual token in the client.", "クライアントで YOUR_MCP_TOKEN を実際のトークンに置き換えてください。", "Sustituye YOUR_MCP_TOKEN por el token real en el cliente."],
"不同客户端的配置格式可能不同；支持表单接入时直接填写地址和认证信息。": ["Configuration formats vary by client. If your client provides a form, enter the URL and authentication details there.", "設定形式はクライアントによって異なります。入力フォームがある場合は URL と認証情報を入力してください。", "El formato depende del cliente. Si ofrece un formulario, introduce allí la URL y la autenticación."],
"复制配置": ["Copy configuration", "設定をコピー", "Copiar configuración"],
"JSON 配置示例": ["JSON configuration example", "JSON 設定例", "Ejemplo de configuración JSON"],
"请使用部署时配置的 MCP 令牌。此页面不读取或展示令牌。": ["Use the MCP token configured for this deployment. This page does not read or display tokens.", "デプロイ時に設定した MCP トークンを使用してください。このページではトークンを取得・表示しません。", "Usa el token MCP configurado en el despliegue. Esta página no lee ni muestra tokens."],
"在客户端添加地址后，按提示完成 OAuth 授权。": ["Add the URL in your client, then follow the OAuth authorization flow.", "クライアントに URL を追加し、OAuth 認証の案内に従ってください。", "Añade la URL en el cliente y sigue los pasos de autorización OAuth."],
"服务已停用，启用后才能接收新请求。": ["This server is disabled. Enable it to accept new requests.", "サービスは停止中です。再開すると新しいリクエストを受け付けます。", "El servicio está desactivado. Actívalo para aceptar nuevas solicitudes."],
"无法自动复制，请选中文本手动复制。": ["Could not copy. Select the text and copy it manually.", "コピーできませんでした。テキストを選択してコピーしてください。", "No se pudo copiar. Selecciona el texto y cópialo manualmente."],
"已复制。": ["Copied.", "コピーしました。", "Copiado."],
"停用后将停止接收新请求，已受理的操作可能完成。可随时重新启用。": ["Disabling stops new requests. Accepted operations may finish. You can enable the server again at any time.", "停止すると新しいリクエストを受け付けません。受付済みの処理は完了する場合があります。いつでも再開できます。", "Al desactivar se rechazan nuevas solicitudes. Las operaciones aceptadas pueden completarse. Puedes reactivarlo en cualquier momento."],
"确认停用": ["Disable", "停止する", "Desactivar"],
"停用服务": ["Disable server", "サービスを停止", "Desactivar servicio"],
"令牌认证": ["Token authentication", "トークン認証", "Autenticación con token"],
"工具 {0}": ["Tools {0}", "ツール {0}", "Herramientas {0}"],
"更多操作": ["More actions", "その他の操作", "Más acciones"],
"开放工具": ["Published tools", "公開ツール", "Herramientas publicadas"],
"接入配置": ["Connection setup", "接続設定", "Configurar conexión"],
"关闭": ["Close", "閉じる", "Cerrar"],
"MCP 服务管理": ["MCP server management", "MCP サービス管理", "Gestión de servicios MCP"],
"正在加载…": ["Loading…", "読み込み中…", "Cargando…"],
"{0} 个服务": ["{0} servers", "{0} 個のサービス", "{0} servicios"],
"创建 MCP 服务": ["Create MCP server", "MCP サービスを作成", "Crear servicio MCP"],
"编辑 MCP 服务": ["Edit MCP server", "MCP サービスを編集", "Editar servicio MCP"],
"请取消编辑、刷新列表后重试。": ["Cancel editing, refresh the list and try again.", "編集をキャンセルし、一覧を更新して再試行してください。", "Cancela la edición, actualiza la lista e inténtalo de nuevo."],
"操作结果待确认，请重试原操作；不要重复创建服务。": ["The result is unconfirmed. Retry the same operation; do not create another server.", "結果が未確認です。同じ操作を再試行し、サービスを重複作成しないでください。", "Resultado sin confirmar. Reintenta la misma operación; no crees otro servicio."],
"服务名称": ["Server name", "サービス名", "Nombre del servicio"],
"启用": ["Enabled", "有効", "Habilitado"],
"暂无能力，请先添加连接。": ["No capabilities yet. Add a connection first.", "機能がありません。先に接続を追加してください。", "No hay capacidades. Añade primero una conexión."],
"服务共用部署的认证配置；开放能力分别管理。保存后立即生效。": ["Servers share deployment authentication; each has its own tool scope. Saving takes effect immediately.", "認証設定は共通ですが、公開機能はサービスごとに管理します。保存すると即時反映されます。", "Los servicios comparten autenticación; cada uno tiene su propio conjunto de herramientas. Guardar aplica los cambios."],
"新增服务当前支持工具；资源、提示词和任务收件箱仍由默认服务提供。": ["New servers currently support tools. Resources, prompts and the task inbox remain on the default server.", "新規サービスは現在ツールに対応しています。リソース、プロンプト、タスク受信箱は既定サービスで提供します。", "Los nuevos servicios admiten herramientas. Recursos, prompts y tareas siguen en el servicio predeterminado."],
"取消": ["Cancel", "キャンセル", "Cancelar"],
"发布历史与调用验证": ["Publication history and call verification", "公開履歴と呼び出し検証", "Historial de publicación y verificación"],
"删除 MCP 服务": ["Delete MCP server", "MCP サービスを削除", "Eliminar servicio MCP"],
"删除后，该地址将停止接收新请求；已受理的操作可能完成。底层连接和历史记录保留。": ["This address will stop accepting new requests. Accepted operations may finish. Connections and history are retained.", "このアドレスは新規リクエストを受け付けなくなります。受付済みの操作は完了する場合があります。接続と履歴は保持されます。", "La dirección dejará de aceptar solicitudes nuevas. Las operaciones aceptadas pueden finalizar. Se conservan conexiones e historial."],
"确认删除": ["Confirm deletion", "削除を確認", "Confirmar eliminación"],
"{0} 项能力": ["{0} capabilities", "{0} 個の機能", "{0} capacidades"],
"编辑": ["Edit", "編集", "Editar"],
"删除": ["Delete", "削除", "Eliminar"],
"尚未创建 MCP 服务": ["No MCP servers yet", "MCP サービスがありません", "Aún no hay servicios MCP"],
"服务已保存。": ["Server saved.", "サービスを保存しました。", "Servicio guardado."],
"服务已删除。": ["Server deleted.", "サービスを削除しました。", "Servicio eliminado."],
"已启用": ["Enabled", "有効", "Habilitado"],
"已停用": ["Disabled", "無効", "Deshabilitado"],
"保存并生效": ["Save and apply", "保存して適用", "Guardar y aplicar"],
"配置 MCP 服务与开放范围。": ["Configure MCP servers and their capabilities.", "MCP サービスと公開範囲を設定します。", "Configura servicios MCP y sus capacidades."],
"统一管理软件、设备与 Agent 的连接。": ["Manage software, device and agent connections in one place.", "ソフトウェア、デバイス、Agent の接続を一元管理します。", "Gestiona conexiones de software, dispositivos y agentes en un solo lugar."],
"← 返回连接": ["← Back to connections", "← 接続に戻る", "← Volver a conexiones"],
"使用指引": ["Quick guide", "使い方ガイド", "Guía de uso"],
"继续配置你的连接": ["Continue setting up your connections", "接続の設定を続ける", "Continúa configurando tus conexiones"],
"从一个连接开始": ["Start with a connection", "接続を追加して始める", "Empieza con una conexión"],
"连接决定接入什么，路由决定消息去哪里。MCP 是面向 Agent 的连接。": ["Connections define what you connect; routes define where messages go. MCP connects agents.", "接続は連携先を、ルートはメッセージの送り先を定義します。MCP は Agent 向けの接続です。", "Las conexiones definen qué conectas; las rutas, adónde van los mensajes. MCP conecta agentes."],
"添加软件或设备": ["Add software or a device", "ソフトウェアやデバイスを追加", "Añade software o un dispositivo"],
"配置接口地址，并说明它能执行什么操作。": ["Set the endpoint and describe the operations it supports.", "エンドポイントを設定し、実行できる操作を説明します。", "Configura el endpoint y describe sus operaciones."],
"配置 MCP 连接": ["Configure the MCP connection", "MCP 接続を設定", "Configura la conexión MCP"],
"选择允许 Agent 使用的能力，预览并发布。": ["Select the capabilities agents can use, then review and publish.", "Agent に許可する機能を選び、確認して公開します。", "Selecciona las capacidades para agentes, revisa y publica."],
"在 Agent 中接入": ["Connect from your agent", "Agent から接続", "Conecta desde tu agente"],
"复制 MCP 地址，在客户端授权并验证调用。": ["Copy the MCP URL, authorize the client and verify a call.", "MCP アドレスをコピーし、クライアントで認可して呼び出しを確認します。", "Copia la URL MCP, autoriza el cliente y verifica una llamada."],
"能力已发布，下一步在 Agent 中配置 MCP 地址。": ["Capabilities are published. Next, set the MCP URL in your agent.", "機能は公開済みです。次に Agent に MCP アドレスを設定します。", "Capacidades publicadas. Configura ahora la URL MCP en tu agente."],
"已有可配置的能力，下一步选择开放范围。": ["Capabilities are available. Next, choose what agents can access.", "設定可能な機能があります。次に公開範囲を選びます。", "Hay capacidades disponibles. Elige qué pueden usar los agentes."],
"点击下方“添加连接”，接入第一个 HTTP 服务。": ["Use “Add connection” below to connect your first HTTP service.", "下の「接続を追加」から最初の HTTP サービスを接続します。", "Usa «Añadir conexión» para conectar tu primer servicio HTTP."],
"选择并发布能力": ["Select and publish capabilities", "機能を選択して公開", "Selecciona y publica capacidades"],
"只转发消息，或交给 Agent 处理？": ["Forward messages or ask an agent to process them?", "メッセージを転送する？ Agent に処理を依頼する？", "¿Reenviar mensajes o pedir que un agente los procese?"],
"转发": ["Forward", "転送", "Reenviar"],
"入口连接 → 路由 → 出口连接": ["Inbound connection → Route → Outbound connection", "受信接続 → ルート → 送信接続", "Conexión de entrada → Ruta → Conexión de salida"],
"例如：Webhook 接收告警，通过邮件连接发送。无需 MCP。": ["Example: receive an alert via webhook and send it by email. No MCP required.", "例：Webhook でアラートを受け取り、メールで送信します。MCP は不要です。", "Ejemplo: recibe una alerta por webhook y envíala por correo. No requiere MCP."],
"Agent 处理": ["Agent processing", "Agent による処理", "Procesamiento por agente"],
"入口连接 → 任务 → MCP 连接 ↔ Agent → 结果投递": ["Inbound connection → Task → MCP connection ↔ Agent → Result delivery", "受信接続 → タスク → MCP 接続 ↔ Agent → 結果配信", "Conexión de entrada → Tarea → Conexión MCP ↔ Agente → Entrega del resultado"],
"Agent 通过 MCP 领取并提交结果，再按规则回复原会话或指定渠道。": ["An agent claims work and submits results via MCP; rules send replies to the source conversation or a chosen channel.", "Agent は MCP でタスクを取得して結果を提出します。ルールに従って元の会話や指定先に返信します。", "El agente recibe tareas y entrega resultados por MCP; las reglas envían la respuesta a la conversación original o al canal elegido."],
"消息路由的在线配置尚未开放。MCP 接入不代表 Agent 会自动领取任务。": ["Online message routing configuration is not available yet. An MCP connection does not mean an agent will claim tasks automatically.", "メッセージルートのオンライン設定は未対応です。MCP 接続だけで Agent がタスクを自動取得するわけではありません。", "La configuración de rutas de mensajes aún no está disponible. Una conexión MCP no implica que el agente recoja tareas automáticamente."],
"已配置连接": ["Configured connections", "設定済みの接続", "Conexiones configuradas"],
"同一个连接可以用于接收、发送或调用，具体取决于它支持的能力。": ["A connection can receive, send or expose operations, depending on its capabilities.", "対応する機能に応じて、同じ接続で受信・送信・操作の公開ができます。", "Una conexión puede recibir, enviar u ofrecer operaciones, según sus capacidades."],
"Agent 连接": ["Agent connection", "Agent 接続", "Conexión de agente"],
"向 Agent 提供已发布能力，接收调用并返回结果。": ["Expose published capabilities to agents, receive calls and return results.", "公開済みの機能を Agent に提供し、呼び出しを受けて結果を返します。", "Ofrece capacidades publicadas a los agentes, recibe llamadas y devuelve resultados."],
"接收调用": ["Receive calls", "呼び出しを受信", "Recibe llamadas"],
"返回结果": ["Return results", "結果を返す", "Devuelve resultados"],
"配置开放范围": ["Configure access", "公開範囲を設定", "Configura el acceso"],
"还没有接入软件或设备。添加 HTTP 服务后，可通过 MCP 向 Agent 开放能力。": ["No software or devices connected yet. Add an HTTP service to expose its capabilities via MCP.", "ソフトウェアやデバイスは未接続です。HTTP サービスを追加すると MCP で機能を公開できます。", "Aún no hay software o dispositivos conectados. Añade un servicio HTTP para ofrecer sus capacidades por MCP."],
"MCP 服务": ["MCP services", "MCP サービス", "Servicios MCP"],
"配置已加载": ["Configuration loaded", "設定読み込み済み", "Configuración cargada"],
"接入软件与设备，管理它们提供的能力。": ["Connect software and devices, and manage their capabilities.", "ソフトウェアやデバイスを接続し、利用できる機能を管理します。", "Conecta software y dispositivos y gestiona sus capacidades."],
"选择向 Agent 开放的能力，并配置接入方式。": ["Choose the capabilities available to agents and configure access.", "Agent に公開する機能を選択し、接続方法を設定します。", "Elige las capacidades disponibles para los agentes y configura el acceso."],
"当前可添加 HTTP 接口，并描述可调用的操作。": ["Add an HTTP endpoint and describe its callable operations.", "HTTP エンドポイントを追加し、呼び出せる操作を定義します。", "Añade un endpoint HTTP y describe las operaciones disponibles."],
"可调用": ["Callable", "呼び出し可能", "Invocable"],
"MCP 客户端需独立授权，发布后需重新读取工具目录。": ["MCP clients require separate authorization and must reload the catalog after publication.", "MCP クライアントには独立した認可が必要です。公開後にカタログを再取得してください。", "Los clientes MCP requieren autorización independiente y deben recargar el catálogo tras publicar."],
"正在读取工作空间…": ["Loading workspace…", "ワークスペースを読み込み中…", "Cargando espacio…"],
"当前请求未获管理授权，请检查部署端认证配置。": ["Management access is not authorized. Check the deployment authentication configuration.", "管理アクセスが認可されていません。デプロイ環境の認証設定を確認してください。", "Acceso de administración no autorizado. Revisa la autenticación del despliegue."],
"配置暂不可用": ["Configuration unavailable", "設定を利用できません", "Configuración no disponible"],
"读取工作空间后即可管理连接与 Agent 能力。": ["Connections and agent capabilities will be available after the workspace loads.", "ワークスペースを読み込むと接続と Agent の機能を管理できます。", "Podrás gestionar conexiones y capacidades tras cargar el espacio."],
"配置未就绪": ["Not ready", "設定未準備", "No disponible"],
"等待工作空间": ["Waiting for workspace", "ワークスペース待機中", "Esperando el espacio"],
"查看软件连接": ["View connections", "ソフトウェア接続を表示", "Ver conexiones"],

  "请填写 JSON 对象。": ["Enter a JSON object.", "JSON オブジェクトを入力してください。", "Introduce un objeto JSON."],
  "工作台": [
    "Overview",
    "ダッシュボード",
    "Inicio"
  ],
  "工作台概览": [
    "Workspace overview",
    "ワークスペース概要",
    "Resumen del espacio"
  ],
  "工作空间": [
    "Workspace",
    "ワークスペース",
    "Espacio de trabajo"
  ],
  "我的工作空间": [
    "My workspace",
    "マイワークスペース",
    "Mi espacio"
  ],
  "当前管理空间": [
    "Current workspace",
    "現在のワークスペース",
    "Espacio actual"
  ],
  "连接后加载空间": [
    "Connect to load workspace",
    "接続後に読み込み",
    "Conecta para cargar"
  ],
  "主导航": [
    "Main navigation",
    "メインナビゲーション",
    "Navegación principal"
  ],
  "跳转到内容": [
    "Skip to content",
    "本文へ移動",
    "Saltar al contenido"
  ],
  "管理台": [
    "Console",
    "管理コンソール",
    "Consola"
  ],
  "连接与能力": [
    "Connections",
    "接続と機能",
    "Conexiones"
  ],
  "Agent 接入": [
    "MCP services",
    "MCP サービス",
    "Servicios MCP"
  ],
  "服务连接": [
    "Service connection",
    "サービス接続",
    "Conexión al servicio"
  ],
  "设置": [
    "Settings",
    "設定",
    "Ajustes"
  ],
  "连接软件与智能": [
    "Software meets intelligence",
    "ソフトウェアと AI をつなぐ",
    "Software e inteligencia"
  ],
  "明确开放范围": [
    "Control what you expose",
    "公開範囲を明確に",
    "Controla lo que compartes"
  ],
  "让每次调用都有边界": [
    "Every call, within bounds",
    "すべての呼び出しに境界を",
    "Cada llamada, con límites"
  ],
  "待连接服务": [
    "Not connected",
    "未接続",
    "Sin conexión"
  ],
  "服务已连接": [
    "Service connected",
    "サービスに接続済み",
    "Servicio conectado"
  ],
  "空间已停用": [
    "Workspace suspended",
    "ワークスペース停止中",
    "Espacio suspendido"
  ],
  "切换深色模式": [
    "Switch to dark mode",
    "ダークモードに切り替え",
    "Cambiar a modo oscuro"
  ],
  "切换浅色模式": [
    "Switch to light mode",
    "ライトモードに切り替え",
    "Cambiar a modo claro"
  ],
  "Synquay · AI 与连接": [
    "Synquay · AI & Connections",
    "Synquay · AI と接続",
    "Synquay · IA y conexiones"
  ],
  "连接 · 开放 · 协作": [
    "Connect · Expose · Collaborate",
    "接続 · 公開 · 協働",
    "Conectar · Compartir · Colaborar"
  ],
  "管理软件连接与能力，让协作从这里开始。": [
    "Your connections and capabilities, in one place.",
    "接続と機能の管理を、ここから。",
    "Tus conexiones y capacidades en un solo lugar."
  ],
  "软件与 Agent 的连接中心": [
    "THE BRIDGE TO YOUR AGENTS",
    "ソフトウェアと Agent の接続拠点",
    "EL PUENTE HACIA TUS AGENTES"
  ],
  "让软件的能力，": [
    "Turn software capabilities",
    "ソフトウェアの機能を、",
    "Convierte capacidades"
  ],
  "成为 Agent 的行动。": [
    "into agent actions.",
    "Agent のアクションへ。",
    "en acciones de agentes."
  ],
  "连接服务，定义能力，掌握开放范围。": [
    "Connect services. Define capabilities. Control access.",
    "サービスを接続し、機能と公開範囲を定義。",
    "Conecta servicios, define capacidades y controla el acceso."
  ],
  "从一个连接开始，搭建你的智能工作空间。": [
    "Build your workspace, one connection at a time.",
    "最初の接続から、ワークスペースを構築。",
    "Construye tu espacio, conexión a conexión."
  ],
  "连接第一个服务": [
    "Connect a service",
    "サービスを接続",
    "Conectar un servicio"
  ],
  "管理我的连接": [
    "Manage connections",
    "接続を管理",
    "Gestionar conexiones"
  ],
  "能力接入流程": [
    "CAPABILITY FLOW",
    "機能の接続フロー",
    "FLUJO DE CAPACIDADES"
  ],
  "接入流程示意：软件接口通过 Synquay 向 Agent 提供能力": [
    "Diagram: software APIs expose capabilities to agents through Synquay",
    "ソフトウェア API が Synquay を通じて Agent に機能を提供する図",
    "Diagrama: las API ofrecen capacidades a los agentes mediante Synquay"
  ],
  "软件接口": [
    "Software API",
    "ソフトウェア API",
    "API de software"
  ],
  "能力与权限": [
    "Capabilities & access",
    "機能と権限",
    "Capacidades y acceso"
  ],
  "按需开放 · 授权访问": [
    "Selected capabilities · Authorized access",
    "必要な機能を公開 · 認可されたアクセス",
    "Capacidades seleccionadas · Acceso autorizado"
  ],
  "已配置能力": [
    "Configured capabilities",
    "設定済み機能",
    "Capacidades configuradas"
  ],
  "已向 Agent 开放": [
    "Available to agents",
    "Agent に公開済み",
    "Disponibles para agentes"
  ],
  "发布状态": [
    "Publication status",
    "公開状態",
    "Estado de publicación"
  ],
  "项": [
    "items",
    "件",
    "elementos"
  ],
  "项能力": [
    "capabilities",
    "件の機能",
    "capacidades"
  ],
  "来自当前空间的软件连接": [
    "From this workspace's connections",
    "現在のワークスペースの接続から",
    "De las conexiones de este espacio"
  ],
  "连接服务后读取配置": [
    "Connect to load configuration",
    "接続後に設定を読み込み",
    "Conecta para cargar la configuración"
  ],
  "当前已发布的能力范围": [
    "Currently published capabilities",
    "現在公開されている機能",
    "Capacidades publicadas actualmente"
  ],
  "发布后可供客户端发现": [
    "Discoverable after publication",
    "公開後にクライアントから検出可能",
    "Visibles tras su publicación"
  ],
  "依据当前草稿与发布版本": [
    "Based on draft and published versions",
    "下書きと公開バージョンに基づく",
    "Según borrador y versión publicada"
  ],
  "尚未读取服务数据": [
    "Service data not loaded yet",
    "サービスデータ未読み込み",
    "Datos del servicio aún sin cargar"
  ],
  "待连接": [
    "Not connected",
    "未接続",
    "Sin conexión"
  ],
  "已同步": [
    "In sync",
    "同期済み",
    "Sincronizado"
  ],
  "未保存": [
    "Unsaved",
    "未保存",
    "Sin guardar"
  ],
  "待发布": [
    "Unpublished",
    "未公開",
    "Sin publicar"
  ],
  "开始搭建": [
    "Get started",
    "はじめに",
    "Primeros pasos"
  ],
  "接入工作流": [
    "Connection workflow",
    "接続ワークフロー",
    "Flujo de conexión"
  ],
  "三个步骤，完成一次能力接入": [
    "Three steps to connect a capability",
    "3 ステップで機能を接続",
    "Tres pasos para conectar una capacidad"
  ],
  "首次设置": [
    "Initial setup",
    "初期設定",
    "Configuración inicial"
  ],
  "配置指引": [
    "Setup guide",
    "設定ガイド",
    "Guía de configuración"
  ],
  "连接管理服务": [
    "Connect the console",
    "管理サービスに接続",
    "Conectar la consola"
  ],
  "配置访问凭据，加载你的工作空间。": [
    "Enter your credentials to load your workspace.",
    "認証情報を入力してワークスペースを読み込みます。",
    "Introduce tus credenciales para cargar el espacio."
  ],
  "配置服务连接": [
    "Configure connection",
    "接続を設定",
    "Configurar conexión"
  ],
  "添加软件与能力": [
    "Add software capabilities",
    "ソフトウェアと機能を追加",
    "Añadir software y capacidades"
  ],
  "填写接口地址，说明软件可以完成的任务。": [
    "Set an endpoint and describe what it can do.",
    "エンドポイントと機能の説明を設定します。",
    "Define el endpoint y describe lo que puede hacer."
  ],
  "开放给你的 Agent": [
    "Make it available to agents",
    "Agent に公開",
    "Compartir con tus agentes"
  ],
  "发布所需能力，通过 MCP 接入 AI 客户端。": [
    "Publish selected capabilities and connect via MCP.",
    "必要な機能を公開し、MCP で AI に接続します。",
    "Publica capacidades y conecta mediante MCP."
  ],
  "待完成前序设置": [
    "Complete prior steps first",
    "前のステップを完了してください",
    "Completa los pasos anteriores"
  ],
  "添加软件连接": [
    "Add a connection",
    "接続を追加",
    "Añadir una conexión"
  ],
  "配置 HTTP 接口与能力说明，让服务可被理解。": [
    "Define an HTTP endpoint and describe its capability.",
    "HTTP エンドポイントと機能説明を定義します。",
    "Define un endpoint HTTP y describe su capacidad."
  ],
  "管理连接": [
    "Manage connections",
    "接続を管理",
    "Gestionar conexiones"
  ],
  "添加连接": [
    "Add connection",
    "接続を追加",
    "Añadir conexión"
  ],
  "选择开放的能力": [
    "Choose capabilities to expose",
    "公開する機能を選択",
    "Elegir capacidades para compartir"
  ],
  "选择 Agent 可见的能力，检查变更并发布。": [
    "Choose agent-visible capabilities, review and publish.",
    "Agent に公開する機能を選び、確認して公開します。",
    "Elige capacidades, revisa los cambios y publica."
  ],
  "选择能力": [
    "Choose capabilities",
    "機能を選択",
    "Elegir capacidades"
  ],
  "接入你的 Agent": [
    "Connect your agent",
    "Agent を接続",
    "Conectar tu agente"
  ],
  "获取 MCP 地址，在客户端授权并验证调用。": [
    "Get the MCP URL, authorize and verify a call.",
    "MCP URL を取得し、認可して呼び出しを検証します。",
    "Obtén la URL MCP, autoriza y verifica una llamada."
  ],
  "查看接入方式": [
    "View connection details",
    "接続方法を確認",
    "Ver cómo conectar"
  ],
  "由你决定，开放什么": [
    "You decide what to expose",
    "何を公開するかは、あなた次第",
    "Tú decides qué compartir"
  ],
  "连接软件后，只有选中并发布的能力才会出现在 Agent 的目录中。": [
    "Only selected and published capabilities appear in the agent's catalog.",
    "選択して公開した機能だけが Agent のカタログに表示されます。",
    "Solo las capacidades seleccionadas y publicadas aparecen en el catálogo del agente."
  ],
  "先检查变更，再确认发布": [
    "Review changes before publishing",
    "変更を確認してから公開",
    "Revisa los cambios antes de publicar"
  ],
  "每次调用仍需通过授权": [
    "Every call still requires authorization",
    "呼び出しごとに認可が必要",
    "Cada llamada requiere autorización"
  ],
  "连接配置与能力发布分开管理，": [
    "Manage connections and publication separately.",
    "接続設定と機能公開を分けて管理。",
    "Gestiona conexiones y publicaciones por separado."
  ],
  "开放范围始终清晰可见。": [
    "Keep your access scope clear.",
    "公開範囲をいつでも明確に。",
    "Mantén claro el alcance del acceso."
  ],
  "服务凭据已失效，请更新后重新连接。": [
    "Service credentials expired. Update them and reconnect.",
    "認証情報が失効しました。更新して再接続してください。",
    "Las credenciales han caducado. Actualízalas y vuelve a conectar."
  ],
  "已断开管理服务，页面中的凭据已清除。": [
    "Disconnected. Credentials have been cleared from this page.",
    "切断しました。このページの認証情報は消去されました。",
    "Desconectado. Se borraron las credenciales de esta página."
  ],
  "连接服务与能力": [
    "Connections & capabilities",
    "接続と機能",
    "Conexiones y capacidades"
  ],
  "尚未连接管理服务": [
    "Console not connected",
    "管理サービス未接続",
    "Consola sin conexión"
  ],
  "连接管理服务后查看": [
    "Connect the service to view",
    "サービスに接続して表示：",
    "Conecta el servicio para ver"
  ],
  "连接配置": [
    "Connection configuration",
    "接続設定",
    "Configuración de conexiones"
  ],
  "Agent 接入配置": [
    "MCP services configuration",
    "MCP サービス設定",
    "Configuración del agente"
  ],
  "当前尚未读取服务数据。": [
    "Service data has not been loaded yet.",
    "サービスデータはまだ読み込まれていません。",
    "Aún no se han cargado datos del servicio."
  ],
  "服务连接设置": [
    "Service connection settings",
    "サービス接続設定",
    "Ajustes de conexión"
  ],
  "连接后可读取配置、管理连接和发布 Agent 能力。": [
    "Connect to read configuration, manage connections and publish capabilities.",
    "接続すると設定の読み込み、接続管理、機能公開ができます。",
    "Conecta para leer ajustes, gestionar conexiones y publicar capacidades."
  ],
  "管理员令牌": [
    "Administrator token",
    "管理者トークン",
    "Token de administrador"
  ],
  "令牌仅保存在当前页面内存中，刷新后需要重新输入。": [
    "The token stays in page memory only. Re-enter it after reloading.",
    "トークンはページのメモリ内のみ。再読み込み後は再入力が必要です。",
    "El token solo se guarda en memoria. Introdúcelo de nuevo tras recargar."
  ],
  "请求编号：": [
    "Request ID: ",
    "リクエスト ID：",
    "ID de solicitud: "
  ],
  "正在连接…": [
    "Connecting…",
    "接続中…",
    "Conectando…"
  ],
  "连接服务": [
    "Connect service",
    "サービスに接続",
    "Conectar servicio"
  ],
  "首次使用": [
    "First-time setup",
    "初回設定",
    "Primera configuración"
  ],
  "启用工具配置和管理员配置后启动后端，再输入管理员令牌。MCP 客户端令牌不能用于管理台。": [
    "Enable tool and administrator configuration, start the backend, then enter the admin token. MCP client tokens cannot access this console.",
    "ツールと管理者設定を有効にしてバックエンドを起動し、管理者トークンを入力してください。MCP 用トークンは使用できません。",
    "Activa la configuración de herramientas y administrador, inicia el backend e introduce el token de administrador. Los tokens MCP no sirven para la consola."
  ],
  "管理员配置的": [
    "The administrator setting",
    "管理者設定の",
    "La opción de administrador"
  ],
  "需包含当前页面地址：": [
    "must include this page's origin: ",
    "にこのページのオリジンを含めてください：",
    "debe incluir el origen de esta página: "
  ],
  "只读": [
    "Read only",
    "読み取り専用",
    "Solo lectura"
  ],
  "有副作用": [
    "Changes state",
    "状態を変更",
    "Modifica el estado"
  ],
  "未声明": [
    "Unspecified",
    "未指定",
    "Sin especificar"
  ],
  "详情": [
    "Details",
    "詳細",
    "Detalles"
  ],
  "连接标识": [
    "Connection ID",
    "接続 ID",
    "ID de conexión"
  ],
  "上游地址": [
    "Upstream URL",
    "上流 URL",
    "URL de origen"
  ],
  "凭据引用": [
    "Credential reference",
    "認証情報の参照",
    "Referencia de credencial"
  ],
  "无": [
    "None",
    "なし",
    "Ninguno"
  ],
  "输入格式": [
    "Input schema",
    "入力形式",
    "Esquema de entrada"
  ],
  "输出格式": [
    "Output schema",
    "出力形式",
    "Esquema de salida"
  ],
  "当前空间": [
    "Current workspace",
    "現在のワークスペース",
    "Espacio actual"
  ],
  "服务配置": [
    "Service configuration",
    "サービス設定",
    "Configuración del servicio"
  ],
  "能力发布": [
    "Publication",
    "機能の公開",
    "Publicación"
  ],
  "发布预览": [
    "Publication preview",
    "公開プレビュー",
    "Vista previa"
  ],
  "管理当前页面与后台服务的连接。": [
    "Manage this page's connection to the backend.",
    "このページとバックエンドの接続を管理します。",
    "Gestiona la conexión de esta página con el backend."
  ],
  "连接一个服务，让 Agent 安全地使用它。": [
    "Connect a service for authorized agent access.",
    "サービスを接続し、認可された Agent に公開します。",
    "Conecta un servicio para el acceso autorizado de agentes."
  ],
  "管理服务地址、协议与能力说明。": [
    "Manage endpoints, protocols and capability descriptions.",
    "アドレス、プロトコル、機能説明を管理します。",
    "Gestiona endpoints, protocolos y descripciones de capacidades."
  ],
  "选择可供 AI 使用的能力，检查后发布。": [
    "Choose capabilities for AI, review and publish.",
    "AI に公開する機能を選び、確認して公開します。",
    "Elige capacidades para la IA, revisa y publica."
  ],
  "当前视图 · v": [
    "Current view · v",
    "現在の表示 · v",
    "Vista actual · v"
  ],
  "断开连接确认": [
    "Confirm disconnection",
    "切断の確認",
    "Confirmar desconexión"
  ],
  "断开连接会清除页面中的令牌与未保存编辑。已接纳的发布会继续执行，其操作编号会留在当前浏览器标签页，重新连接后可查询。": [
    "Disconnecting clears the token and unsaved edits. Accepted publications continue; operation IDs remain in this tab for recovery after reconnecting.",
    "切断するとトークンと未保存の編集が消去されます。受理済みの公開は継続し、操作 ID はこのタブに残り、再接続後に確認できます。",
    "Al desconectar se borran el token y los cambios sin guardar. Las publicaciones aceptadas continúan; sus ID permanecen en esta pestaña para consultarlos al reconectar."
  ],
  "继续编辑": [
    "Keep editing",
    "編集を続ける",
    "Seguir editando"
  ],
  "断开连接": [
    "Disconnect",
    "切断",
    "Desconectar"
  ],
  "已确认发布 v": [
    "Publication confirmed: v",
    "公開確認済み v",
    "Publicación confirmada: v"
  ],
  "操作编号：": [
    "Operation ID: ",
    "操作 ID：",
    "ID de operación: "
  ],
  "当前视图需要更新。读取最新版本会保留你的选择，请核对后重新保存。": [
    "Refresh the current view. Your selection will be kept; review it before saving again.",
    "表示を更新してください。選択は保持されます。確認後に再保存してください。",
    "Actualiza la vista. Se conservará tu selección; revísala antes de guardar."
  ],
  "读取最新版本，保留选择": [
    "Refresh, keep selection",
    "選択を保持して更新",
    "Actualizar sin perder selección"
  ],
  "发布结果待确认": [
    "Publication unconfirmed",
    "公開結果の確認待ち",
    "Publicación sin confirmar"
  ],
  "操作可能仍在处理。核查完成前，暂停编辑与新发布。": [
    "The operation may still be running. Editing and publication are paused until verification completes.",
    "処理中の可能性があります。確認完了まで編集と新規公開を停止します。",
    "La operación puede seguir en curso. La edición y publicación se pausan hasta verificarla."
  ],
  "查询发布结果": [
    "Check publication",
    "公開結果を確認",
    "Consultar publicación"
  ],
  "使用原编号重试": [
    "Retry with original ID",
    "元の ID で再試行",
    "Reintentar con el ID original"
  ],
  "连接状态": [
    "Connection status",
    "接続状態",
    "Estado de conexión"
  ],
  "已连接": [
    "Connected",
    "接続済み",
    "Conectado"
  ],
  "凭据保存": [
    "Credential storage",
    "認証情報の保存",
    "Almacenamiento de credenciales"
  ],
  "仅当前页面内存；刷新或断开连接后清除。": [
    "Page memory only; cleared on reload or disconnect.",
    "ページのメモリ内のみ。再読み込み・切断時に消去。",
    "Solo en memoria; se borra al recargar o desconectar."
  ],
  "MCP 服务中有未保存的选择，请先保存后再添加连接。": [
    "Save your unsaved agent capability selection before adding connections.",
    "接続を追加する前に、Agent 機能の未保存の選択を保存してください。",
    "Guarda la selección pendiente de capacidades antes de añadir conexiones."
  ],
  "服务标识": [
    "Service ID",
    "サービス ID",
    "ID de servicio"
  ],
  "服务详情": [
    "Service details",
    "サービス詳細",
    "Detalles del servicio"
  ],
  "能力": [
    "Capabilities",
    "機能",
    "Capacidades"
  ],
  "接入方式": [
    "Connection details",
    "接続方法",
    "Cómo conectar"
  ],
  "发布历史": [
    "Publication history",
    "公開履歴",
    "Historial de publicaciones"
  ],
  "能力选择": [
    "Capability selection",
    "機能の選択",
    "Selección de capacidades"
  ],
  "选择可见能力": [
    "Visible capabilities",
    "公開する機能",
    "Capacidades visibles"
  ],
  "刷新": [
    "Refresh",
    "更新",
    "Actualizar"
  ],
  "尚未配置工具": [
    "No tools configured",
    "ツール未設定",
    "Sin herramientas configuradas"
  ],
  "先在“连接”中添加 HTTP 能力，再回来选择开放范围。": [
    "Add an HTTP capability under Connections, then choose its visibility here.",
    "「接続」で HTTP 機能を追加し、ここで公開範囲を選択してください。",
    "Añade una capacidad HTTP en Conexiones y después elige aquí su visibilidad."
  ],
  "来源": [
    "Source",
    "接続元",
    "Origen"
  ],
  "操作性质": [
    "Effect",
    "操作の性質",
    "Efecto"
  ],
  "配置": [
    "Configuration",
    "設定",
    "Configuración"
  ],
  "有未保存修改": [
    "Unsaved changes",
    "未保存の変更あり",
    "Cambios sin guardar"
  ],
  "草稿已保存，尚未发布": [
    "Draft saved, not yet published",
    "下書き保存済み、未公開",
    "Borrador guardado, sin publicar"
  ],
  "与当前已发布版本一致": [
    "Matches the published version",
    "公開済みバージョンと一致",
    "Coincide con la versión publicada"
  ],
  "保存草稿": [
    "Save draft",
    "下書きを保存",
    "Guardar borrador"
  ],
  "预览发布": [
    "Preview publication",
    "公開をプレビュー",
    "Vista previa"
  ],
  "勾选能力不会执行工具。最终可调用范围还需满足调用者授权。": [
    "Selecting capabilities does not execute tools. Calls still require caller authorization.",
    "機能の選択では実行されません。呼び出しには呼び出し元の認可が必要です。",
    "Seleccionar capacidades no ejecuta herramientas. Las llamadas requieren autorización."
  ],
  "连接到 AI 客户端": [
    "Connect an AI client",
    "AI クライアントに接続",
    "Conectar un cliente de IA"
  ],
  "MCP 地址": [
    "MCP URL",
    "MCP URL",
    "URL MCP"
  ],
  "地址已复制。": [
    "URL copied.",
    "URL をコピーしました。",
    "URL copiada."
  ],
  "无法自动复制，请选中地址手动复制。": [
    "Copy failed. Select the URL and copy it manually.",
    "コピーできません。URL を選択して手動でコピーしてください。",
    "No se pudo copiar. Selecciona la URL y cópiala manualmente."
  ],
  "复制地址": [
    "Copy URL",
    "URL をコピー",
    "Copiar URL"
  ],
  "认证方式": [
    "Authentication",
    "認証方法",
    "Autenticación"
  ],
  "OAuth 授权": [
    "OAuth authorization",
    "OAuth 認可",
    "Autorización OAuth"
  ],
  "MCP Bearer 令牌": [
    "MCP bearer token",
    "MCP Bearer トークン",
    "Token Bearer MCP"
  ],
  "授权服务": [
    "Authorization server",
    "認可サーバー",
    "Servidor de autorización"
  ],
  "已开放能力": [
    "Published capabilities",
    "公開済み機能",
    "Capacidades publicadas"
  ],
  "暂无": [
    "None yet",
    "まだありません",
    "Ninguna todavía"
  ],
  "传输方式": [
    "Transport",
    "トランスポート",
    "Transporte"
  ],
  "管理台令牌不能用于 MCP 调用。客户端需使用对应授权，并在发布后重新读取工具目录。": [
    "Console tokens cannot call MCP tools. Clients need their own authorization and must reload the catalog after publication.",
    "管理トークンは MCP 呼び出しには使えません。クライアントは専用の認可を使用し、公開後にカタログを再取得してください。",
    "El token de consola no sirve para MCP. Los clientes necesitan su autorización y deben recargar el catálogo tras publicar."
  ],
  "此地址按当前访问地址生成，本地开发代理已转发 /mcp；公网部署需配置同源反向代理和可访问的 HTTPS 地址。": [
    "This URL uses the current origin. The dev proxy forwards /mcp. Public deployment requires a same-origin reverse proxy and an accessible HTTPS URL.",
    "現在のオリジンから生成した URL です。開発プロキシは /mcp を転送します。公開には同一オリジンのリバースプロキシと HTTPS URL が必要です。",
    "La URL usa el origen actual. El proxy de desarrollo reenvía /mcp. El despliegue público requiere proxy inverso del mismo origen y una URL HTTPS accesible."
  ],
  "此页展示接入配置，未验证外部客户端连通性。": [
    "These are connection settings; external client connectivity has not been verified.",
    "接続設定を表示しています。外部クライアントの疎通は未検証です。",
    "Se muestran ajustes; no se ha verificado la conexión de clientes externos."
  ],
  "发布变更预览": [
    "Review publication changes",
    "公開変更の確認",
    "Revisar cambios de publicación"
  ],
  "新增": [
    "Added",
    "追加",
    "Añadidas"
  ],
  "· 移除": [
    "· Removed",
    "· 削除",
    "· Eliminadas"
  ],
  "移除": [
    "Removed",
    "削除",
    "Eliminadas"
  ],
  "保持不变": [
    "Unchanged",
    "変更なし",
    "Sin cambios"
  ],
  "访问控制": [
    "Access control",
    "アクセス制御",
    "Control de acceso"
  ],
  "保留已有调用者授权约束": [
    "Existing caller authorization remains in effect",
    "既存の呼び出し元の認可制約を維持",
    "Se conserva la autorización del solicitante"
  ],
  "客户端影响": [
    "Client impact",
    "クライアントへの影響",
    "Impacto en clientes"
  ],
  "当前版本尚不能计算完整影响范围": [
    "Full impact analysis is not available yet",
    "完全な影響分析は未対応",
    "El análisis completo del impacto aún no está disponible"
  ],
  "发布会等待已接纳的调用结束。发布成功后，后续目录与调用按新版本执行；客户端需要重新查询目录。": [
    "Publication waits for accepted calls to finish. After success, catalogs and calls use the new version. Clients must reload the catalog.",
    "受理済みの呼び出しが終わるまで公開を待機します。成功後は新しいバージョンを使用するため、カタログを再取得してください。",
    "La publicación espera las llamadas aceptadas. Después, catálogos y llamadas usan la nueva versión; los clientes deben recargar el catálogo."
  ],
  "返回编辑": [
    "Back to editing",
    "編集に戻る",
    "Volver a editar"
  ],
  "确认发布": [
    "Confirm publication",
    "公開を確定",
    "Confirmar publicación"
  ],
  "能力名称请使用 1–100 个英文字母、数字、下划线或连字符。": [
    "Use 1–100 letters, digits, underscores or hyphens for the capability name.",
    "機能名には英数字・下線・ハイフンを 1～100 文字で指定してください。",
    "Usa entre 1 y 100 letras, dígitos, guiones o guiones bajos."
  ],
  "地址不能包含账号、密码、查询参数或片段。": [
    "The URL cannot contain credentials, query parameters or fragments.",
    "URL に認証情報、クエリ、フラグメントは指定できません。",
    "La URL no puede incluir credenciales, parámetros ni fragmentos."
  ],
  "请输入 HTTPS 地址；本机 HTTP 地址需要单独允许。": [
    "Enter an HTTPS URL. Local HTTP must be explicitly allowed.",
    "HTTPS URL を入力してください。ローカル HTTP は明示的に許可してください。",
    "Introduce una URL HTTPS. HTTP local requiere permiso explícito."
  ],
  "请说明这个能力能做什么及何时使用。": [
    "Describe what this capability does and when to use it.",
    "機能の用途と使用条件を説明してください。",
    "Describe qué hace esta capacidad y cuándo usarla."
  ],
  "输入格式必须是 type 为 object 的 JSON Schema。": [
    "Input must be a JSON Schema with type object.",
    "入力は type が object の JSON Schema にしてください。",
    "La entrada debe ser un JSON Schema de tipo object."
  ],
  "操作未完成。": [
    "Operation incomplete.",
    "操作が完了していません。",
    "Operación incompleta."
  ],
  "无法保存恢复编号，尚未发起操作。请允许此站点使用会话存储。": [
    "Cannot store the recovery ID; no operation was started. Allow session storage for this site.",
    "復旧 ID を保存できないため未実行です。このサイトのセッションストレージを許可してください。",
    "No se guardó el ID de recuperación; no se inició la operación. Permite el almacenamiento de sesión."
  ],
  "连接管理": [
    "Connection management",
    "接続管理",
    "Gestión de conexiones"
  ],
  "连接": [
    "Connections",
    "接続",
    "Conexiones"
  ],
  "先接入服务，再描述 Agent 可以使用的能力。": [
    "Connect a service, then describe the capabilities agents can use.",
    "サービスを接続して、Agent が使える機能を説明します。",
    "Conecta un servicio y describe las capacidades que pueden usar los agentes."
  ],
  "当前服务尚未提供在线配置接口，请升级并重启后端。": [
    "Online configuration is unavailable. Upgrade and restart the backend.",
    "オンライン設定に未対応です。バックエンドを更新して再起動してください。",
    "La configuración en línea no está disponible. Actualiza y reinicia el backend."
  ],
  "保存结果待确认": [
    "Save result unconfirmed",
    "保存結果の確認待ち",
    "Guardado sin confirmar"
  ],
  "先查询原操作，不要重复添加。恢复编号仅保存在当前标签页；表单和令牌不会保存到浏览器。": [
    "Check the original operation; do not add it again. Only the recovery ID is stored in this tab, not forms or tokens.",
    "再追加せず元の操作を確認してください。復旧 ID のみタブに保存され、フォームやトークンは保存されません。",
    "Consulta la operación original sin duplicarla. Solo se guarda el ID en esta pestaña, no formularios ni tokens."
  ],
  "尚未发现提交回执。可稍后查询；若已重新加载页面，请保留编号并联系管理员核查。": [
    "No receipt found yet. Check later; after a reload, keep the ID and ask an administrator to verify.",
    "受領記録がまだありません。後で確認してください。再読み込み済みなら ID を保持し管理者に確認してください。",
    "No hay recibo aún. Consulta más tarde; si recargaste, conserva el ID y contacta al administrador."
  ],
  "查询保存结果": [
    "Check save result",
    "保存結果を確認",
    "Consultar guardado"
  ],
  "我已核查服务端，结束本地跟踪": [
    "I checked the server; stop local tracking",
    "サーバーを確認済み。ローカル追跡を終了",
    "He revisado el servidor; finalizar seguimiento local"
  ],
  "结束核查": [
    "Finish verification",
    "確認を終了",
    "Finalizar verificación"
  ],
  "这不会取消已经接纳的保存操作。": [
    "This does not cancel an accepted save operation.",
    "受理済みの保存操作はキャンセルされません。",
    "Esto no cancela un guardado ya aceptado."
  ],
  "添加连接 ·": [
    "Add connection ·",
    "接続を追加 ·",
    "Añadir conexión ·"
  ],
  "连接到服务": [
    "Connect to a service",
    "サービスに接続",
    "Conectar a un servicio"
  ],
  "MQTT、CLI 与上游 MCP 的协议抽象已定义，驱动接入尚未开放。当前在线新增仅支持无认证端点；带凭据的服务仍需在服务端配置。": [
    "MQTT, CLI and upstream MCP abstractions are defined; drivers are not available yet. Online creation supports unauthenticated endpoints only. Configure credentials on the server.",
    "MQTT、CLI、上流 MCP の抽象は定義済みですが、ドライバーは未対応です。オンライン追加は認証なしのエンドポイントのみ。認証情報はサーバー側で設定してください。",
    "Las abstracciones MQTT, CLI y MCP están definidas, pero sus controladores no están disponibles. Solo se pueden añadir endpoints sin autenticación; configura credenciales en el servidor."
  ],
  "请求地址": [
    "Request URL",
    "リクエスト URL",
    "URL de solicitud"
  ],
  "允许服务端本机的 HTTP 地址": [
    "Allow HTTP on the server's loopback interface",
    "サーバーのループバック HTTP を許可",
    "Permitir HTTP en loopback del servidor"
  ],
  "描述能力": [
    "Describe the capability",
    "機能を説明",
    "Describir la capacidad"
  ],
  "能力名称": [
    "Capability name",
    "機能名",
    "Nombre de capacidad"
  ],
  "用途与调用条件": [
    "Purpose and usage conditions",
    "用途と呼び出し条件",
    "Uso y condiciones de llamada"
  ],
  "查询设备当前状态。用户询问设备状态时使用，需要提供设备编号。": [
    "Query device status when a user asks about it. Requires a device ID.",
    "ユーザーが状態を尋ねた際にデバイスの状態を照会します。デバイス ID が必要です。",
    "Consulta el estado del dispositivo cuando el usuario lo solicite. Requiere su ID."
  ],
  "只读取数据": [
    "Reads data only",
    "データの読み取りのみ",
    "Solo lee datos"
  ],
  "会修改数据或操作设备": [
    "Modifies data or operates devices",
    "データの変更またはデバイス操作",
    "Modifica datos u opera dispositivos"
  ],
  "输入参数 · JSON Schema": [
    "Input parameters · JSON Schema",
    "入力パラメーター · JSON Schema",
    "Parámetros · JSON Schema"
  ],
  "请按接口实际参数定义。服务端会验证格式；权限仍由空间成员与调用者授权共同决定。": [
    "Match the endpoint's actual parameters. The server validates the schema; workspace membership and caller authorization still apply.",
    "実際のパラメーターを定義してください。形式はサーバーが検証し、メンバー権限と呼び出し元認可が適用されます。",
    "Define los parámetros reales. El servidor valida el esquema; se aplican membresía del espacio y autorización del solicitante."
  ],
  "检查并保存": [
    "Review and save",
    "確認して保存",
    "Revisar y guardar"
  ],
  "用途": [
    "Purpose",
    "用途",
    "Uso"
  ],
  "Agent 可见": [
    "Visible to agents",
    "Agent に公開",
    "Visible para agentes"
  ],
  "默认关闭，保存后另行选择并发布": [
    "Off by default; select and publish after saving",
    "既定では非公開。保存後に選択して公開",
    "Desactivado por defecto; selecciona y publica tras guardar"
  ],
  "输入参数格式": [
    "Input parameter schema",
    "入力パラメーター形式",
    "Esquema de parámetros"
  ],
  "保存只登记连接，不会请求上游服务。下一步可在 MCP 服务中发布能力，并主动验证调用。": [
    "Saving registers the connection without calling the upstream service. Next, publish under MCP services and verify a call explicitly.",
    "保存は接続の登録のみで、上流サービスは呼び出しません。次に MCP サービスで公開し、明示的に呼び出しを検証できます。",
    "Guardar registra la conexión sin llamar al servicio. Después publica en Servicios MCP y verifica una llamada explícitamente."
  ],
  "取消添加": [
    "Cancel",
    "追加をキャンセル",
    "Cancelar"
  ],
  "上一步": [
    "Back",
    "戻る",
    "Anterior"
  ],
  "下一步": [
    "Next",
    "次へ",
    "Siguiente"
  ],
  "正在保存…": [
    "Saving…",
    "保存中…",
    "Guardando…"
  ],
  "保存连接": [
    "Save connection",
    "接続を保存",
    "Guardar conexión"
  ],
  "已开放": [
    "Published",
    "公開済み",
    "Publicada"
  ],
  "未开放": [
    "Not published",
    "未公開",
    "Sin publicar"
  ],
  "还没有连接。添加一个 HTTP 服务开始使用。": [
    "No connections yet. Add an HTTP service to get started.",
    "接続がありません。HTTP サービスを追加してください。",
    "Aún no hay conexiones. Añade un servicio HTTP para empezar."
  ],
  "当前支持最多 32 项 HTTP 能力，每项独立绑定一个端点。在线修改与删除连接将在后续补齐。": [
    "Up to 32 HTTP capabilities, each bound to one endpoint. Online editing and deletion are not yet available.",
    "HTTP 機能は最大 32 件で各機能に 1 つのエンドポイントを設定します。オンライン編集・削除は未対応です。",
    "Hasta 32 capacidades HTTP, cada una con su endpoint. La edición y eliminación en línea aún no están disponibles."
  ],
  "无法保存调用编号，尚未执行。": [
    "Cannot store the invocation ID; nothing was executed.",
    "呼び出し ID を保存できないため未実行です。",
    "No se pudo guardar el ID de llamada; no se ejecutó nada."
  ],
  "调用结果待确认。": [
    "Invocation result unconfirmed.",
    "呼び出し結果の確認待ちです。",
    "Resultado de llamada sin confirmar."
  ],
  "调用已完成，响应通过验证。": [
    "Call completed and response validated.",
    "呼び出し完了。応答の検証に成功しました。",
    "Llamada completada y respuesta validada."
  ],
  "调用未成功，请用调用编号核查服务端记录。": [
    "Call failed. Check server records using the invocation ID.",
    "呼び出しに失敗しました。ID でサーバーの記録を確認してください。",
    "La llamada falló. Consulta los registros con el ID de llamada."
  ],
  "执行结果未知，请核查目标服务，勿重复执行。": [
    "Outcome unknown. Check the target service; do not repeat the call.",
    "結果不明です。対象サービスを確認し、再実行しないでください。",
    "Resultado desconocido. Revisa el servicio; no repitas la llamada."
  ],
  "尚未发现记录；无法据此判断是否执行。请保留编号并核查服务端。": [
    "No record found; execution cannot be determined. Keep the ID and check the server.",
    "記録がありません。実行の有無は判断できません。ID を保持しサーバーを確認してください。",
    "Sin registro; no se puede determinar si se ejecutó. Conserva el ID y revisa el servidor."
  ],
  "验证一次服务内调用": [
    "Verify one internal call",
    "サービス内呼び出しを検証",
    "Verificar una llamada interna"
  ],
  "使用 MCP 调用令牌，经过相同的成员权限、能力授权和执行链路。此验证不会证明外部 AI 客户端已经连通。": [
    "Uses an MCP token with the same membership, authorization and execution path. This does not verify external AI client connectivity.",
    "MCP トークンを使用し、同じメンバー権限・機能認可・実行経路を通ります。外部 AI クライアントの疎通は確認できません。",
    "Usa un token MCP con la misma membresía, autorización y ejecución. No verifica la conexión de clientes externos."
  ],
  "MCP 调用令牌": [
    "MCP invocation token",
    "MCP 呼び出しトークン",
    "Token de llamada MCP"
  ],
  "已发布能力": [
    "Published capability",
    "公開済み機能",
    "Capacidad publicada"
  ],
  "调用参数": [
    "Arguments",
    "呼び出しパラメーター",
    "Argumentos"
  ],
  "我确认此次验证会实际调用目标服务，已检查参数与操作影响": [
    "I understand this calls the real service and have reviewed the arguments and effects",
    "実際にサービスを呼び出すことを理解し、パラメーターと影響を確認しました",
    "Entiendo que se llamará al servicio real y he revisado los argumentos y efectos"
  ],
  "仅查询原调用状态，不会重新执行。重新打开此页后，需再次输入同一调用者的令牌。": [
    "Only checks the original call; it will not execute again. Re-enter the same caller's token after reopening this page.",
    "元の呼び出し状態のみ照会し、再実行しません。再表示後は同じ呼び出し元のトークンを再入力してください。",
    "Solo consulta la llamada original, sin repetirla. Al reabrir, introduce el token del mismo solicitante."
  ],
  "我已在目标服务核查结果，结束本地跟踪": [
    "I verified the result at the target service; stop local tracking",
    "対象サービスで結果を確認済み。ローカル追跡を終了",
    "He verificado el resultado en el servicio; finalizar seguimiento local"
  ],
  "这不会取消已经接纳的调用。": [
    "This does not cancel an accepted call.",
    "受理済みの呼び出しはキャンセルされません。",
    "Esto no cancela una llamada ya aceptada."
  ],
  "正在处理…": [
    "Processing…",
    "処理中…",
    "Procesando…"
  ],
  "查询调用结果": [
    "Check call result",
    "呼び出し結果を確認",
    "Consultar resultado"
  ],
  "执行一次验证": [
    "Execute one test call",
    "検証を 1 回実行",
    "Ejecutar una prueba"
  ],
  "版本记录包含文件发布和管理台发布。": [
    "History includes file-based and console publications.",
    "ファイルと管理台からの公開を含む履歴です。",
    "El historial incluye publicaciones por archivo y consola."
  ],
  "刷新记录": [
    "Refresh history",
    "履歴を更新",
    "Actualizar historial"
  ],
  "版本": [
    "Version",
    "バージョン",
    "Versión"
  ],
  "发布时间": [
    "Published at",
    "公開日時",
    "Fecha de publicación"
  ],
  "开放能力": [
    "Published capabilities",
    "公開機能",
    "Capacidades publicadas"
  ],
  "查看": [
    "View",
    "表示",
    "Ver"
  ],
  "未开放任何能力": [
    "No capabilities published",
    "公開された機能なし",
    "Ninguna capacidad publicada"
  ],
  "操作者": [
    "Actor",
    "操作者",
    "Responsable"
  ],
  "尚无发布记录。": [
    "No publications yet.",
    "公開履歴はまだありません。",
    "Aún no hay publicaciones."
  ],
  "正在读取记录…": [
    "Loading history…",
    "履歴を読み込み中…",
    "Cargando historial…"
  ],
  "加载更早记录": [
    "Load earlier records",
    "以前の履歴を読み込み",
    "Cargar registros anteriores"
  ],
  "操作未完成，请重新查询状态。": [
    "Operation incomplete. Check its status again.",
    "操作未完了です。状態を再確認してください。",
    "Operación incompleta. Consulta el estado de nuevo."
  ],
  "浏览器无法保存发布操作编号。本次发布未发起，请允许当前站点使用会话存储。": [
    "Cannot store the publication ID. Nothing was started. Allow session storage for this site.",
    "公開 ID を保存できないため未開始です。セッションストレージを許可してください。",
    "No se guardó el ID de publicación. No se inició nada. Permite el almacenamiento de sesión."
  ],
  "管理员令牌无效或已失效，请重新连接。": [
    "Administrator token invalid or expired. Reconnect.",
    "管理者トークンが無効または失効しています。再接続してください。",
    "Token de administrador no válido o caducado. Vuelve a conectar."
  ],
  "此管理台地址尚未获准访问，请在管理员配置中添加准确的浏览器来源。": [
    "This console origin is not allowed. Add the exact browser origin to administrator configuration.",
    "この管理台のオリジンは未許可です。管理者設定に正確なオリジンを追加してください。",
    "El origen no está autorizado. Añade el origen exacto a la configuración de administrador."
  ],
  "当前身份没有执行此操作的权限。": [
    "You do not have permission for this operation.",
    "この操作の権限がありません。",
    "No tienes permiso para esta operación."
  ],
  "管理接口未启用或对象不存在，请检查管理员配置。": [
    "The admin API is disabled or the object does not exist. Check configuration.",
    "管理 API が無効、または対象が存在しません。設定を確認してください。",
    "La API está desactivada o el objeto no existe. Revisa la configuración."
  ],
  "配置已发生变化，请读取最新版本后重新检查。": [
    "Configuration changed. Load the latest version and review again.",
    "設定が変更されました。最新バージョンを読み込み、確認してください。",
    "La configuración cambió. Carga la última versión y revísala."
  ],
  "已有操作正在处理或服务繁忙，请稍后查询。": [
    "An operation is running or the service is busy. Check later.",
    "操作中またはサービスが混雑しています。後で確認してください。",
    "Hay una operación en curso o el servicio está ocupado. Consulta más tarde."
  ],
  "存储容量不足，暂时无法保存。": [
    "Storage is full. Cannot save right now.",
    "保存容量が不足しているため保存できません。",
    "Almacenamiento lleno. No se puede guardar ahora."
  ],
  "配置或操作参数无效，请重新读取服务信息。": [
    "Invalid configuration or parameters. Reload service information.",
    "設定またはパラメーターが無効です。サービス情報を再取得してください。",
    "Configuración o parámetros no válidos. Recarga la información."
  ],
  "服务响应格式异常，结果尚未确认。": [
    "Unexpected response format. Outcome is unconfirmed.",
    "応答形式が異常です。結果は未確認です。",
    "Formato de respuesta inesperado. Resultado sin confirmar."
  ],
  "服务暂时无法响应，结果尚未确认。": [
    "Service unavailable. Outcome is unconfirmed.",
    "サービスが応答できません。結果は未確認です。",
    "Servicio no disponible. Resultado sin confirmar."
  ],
  "正在读取最新版本": [
    "Loading latest version",
    "最新バージョンを読み込み中",
    "Cargando última versión"
  ],
  "已读取最新版本并保留你的选择，请重新检查后保存草稿。": [
    "Latest version loaded, selection kept. Review before saving the draft.",
    "最新バージョンを読み込み、選択を保持しました。確認後に保存してください。",
    "Última versión cargada y selección conservada. Revisa antes de guardar."
  ],
  "已读取最新版本。": [
    "Latest version loaded.",
    "最新バージョンを読み込みました。",
    "Última versión cargada."
  ],
  "正在保存草稿": [
    "Saving draft",
    "下書きを保存中",
    "Guardando borrador"
  ],
  "草稿已保存，当前运行版本尚未改变。": [
    "Draft saved. The running version is unchanged.",
    "下書き保存済み。稼働中のバージョンは未変更です。",
    "Borrador guardado. La versión en ejecución no ha cambiado."
  ],
  "正在确认原发布操作": [
    "Checking original publication",
    "元の公開操作を確認中",
    "Comprobando publicación original"
  ],
  "正在发布，等待当前调用完成": [
    "Publishing; waiting for current calls",
    "公開中。実行中の呼び出しの完了待ち",
    "Publicando; esperando llamadas actuales"
  ],
  "正在查询发布结果": [
    "Checking publication result",
    "公開結果を確認中",
    "Consultando publicación"
  ],
  "尚未找到已提交的回执。操作可能仍在处理；稍后查询，或使用原操作编号重试。": [
    "No receipt yet. The operation may be running; check later or retry with the original ID.",
    "受領記録がまだありません。処理中の可能性があります。後で確認するか元の ID で再試行してください。",
    "Sin recibo aún. Puede seguir en curso; consulta más tarde o reintenta con el ID original."
  ],
  "后端已连接": [
    "Backend connected",
    "バックエンドに接続済み",
    "Backend conectado"
  ],
  "暂时无法连接后端": [
    "Backend unavailable",
    "バックエンドに接続できません",
    "Backend no disponible"
  ],
  "正在连接服务…": [
    "Connecting to service…",
    "サービスに接続中…",
    "Conectando al servicio…"
  ],
  "服务连接状态": [
    "Service connection status",
    "サービス接続状態",
    "Estado de conexión"
  ],
  "正在检查服务是否可用。": [
    "Checking service availability.",
    "サービスの稼働状況を確認中。",
    "Comprobando disponibilidad del servicio."
  ],
  "请确认后端正在运行，然后重新检查。": [
    "Ensure the backend is running, then check again.",
    "バックエンドの起動を確認して再試行してください。",
    "Comprueba que el backend funciona y vuelve a intentarlo."
  ],
  "排查编号：": [
    "Diagnostic ID: ",
    "診断 ID：",
    "ID de diagnóstico: "
  ],
  "重新检查": [
    "Check again",
    "再確認",
    "Volver a comprobar"
  ],
  "语言": [
    "Language",
    "言語",
    "Idioma"
  ],
  "Synquay 管理台": [
    "Synquay Console",
    "Synquay 管理コンソール",
    "Consola Synquay"
  ],
  "已显示 {0} 条": [
    "{0} records shown",
    "{0} 件表示",
    "{0} registros mostrados"
  ],
  "发布已确认：v{0}。": [
    "Publication confirmed: v{0}.",
    "公開確認済み：v{0}。",
    "Publicación confirmada: v{0}."
  ],
  "已保存连接（v{0}），尚未向 Agent 开放。前往 MCP 服务，选择能力并发布。": [
    "Connection saved (v{0}), not yet exposed. Go to MCP services to select and publish it.",
    "接続保存済み（v{0}）、まだ未公開です。MCP サービスで選択して公開してください。",
    "Conexión guardada (v{0}), aún sin publicar. Ve a Servicios MCP para seleccionarla y publicarla."
  ],
"状态未知": ["Status unknown", "状態不明", "Estado desconocido"],
"连接中": ["Connecting", "接続中", "Conectando"],
"订阅中": ["Subscribing", "購読中", "Suscribiendo"],
"Broker 已连接": ["Broker connected", "Broker 接続済み", "Broker conectado"],
"重连中": ["Reconnecting", "再接続中", "Reconectando"],
"未连接": ["Disconnected", "未接続", "Desconectado"],
"连接异常": ["Connection error", "接続エラー", "Error de conexión"],
"接收就绪": ["Ready to receive", "受信待機中", "Listo para recibir"],
"按需发送": ["Send on demand", "送信時に接続", "Envío bajo demanda"],
"累计计数；发送表示发送端确认接受。": ["Cumulative counts; sent means accepted by the destination.", "累計件数。送信は送信先の受付確認を意味します。", "Recuentos acumulados; enviado significa aceptado por el destino."],
"接收 {0}": ["Received {0}", "受信 {0}", "Recibidos {0}"],
"发送 {0}": ["Sent {0}", "送信 {0}", "Enviados {0}"],
"匹配 {0}": ["Matched {0}", "一致 {0}", "Coincidencias {0}"],
"匹配数按消息累计，多出口只计一次；投递状态为保留期内记录，已提交不代表设备已执行。": ["Matches count each message once per route across destinations; delivery states cover retained records. Submitted does not mean executed by a device.", "一致数はルートごとに1メッセージ1件です。配信状態は保存期間内の記録で、送信済みは機器の実行完了を意味しません。", "Cada mensaje cuenta una vez por ruta; los estados cubren registros retenidos. Enviado no significa ejecutado por el dispositivo."]
};
