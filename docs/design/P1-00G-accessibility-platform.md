# P1-00G — доступность и платформенное поведение

**Версия:** 1.0, 2026-10-24. **Статус:** принято для P1 handoff. Это design/acceptance contract; фактическое соответствие реализованного UI доказывается в P2-00 и P1-11, а не существованием документа.

## 1. Целевой уровень и область

- Проектируем на WCAG 2.2 AA как минимальную продуктовую планку. Для focus appearance используем более строгую проверяемую форму: видимый perimeter не менее 2 CSS px и контраст изменения не менее 3:1.
- Контракт охватывает bundled Svelte UI в поддерживаемых desktop-браузерах Windows 10+ и заявленных Linux GUI environments. Он не обещает доступность ещё не реализованных экранов.
- Native HTML semantics используются прежде ARIA. Composite widgets следуют WAI-ARIA Authoring Practices; самодельный keyboard model без документированного pattern запрещён.
- Мышь, клавиатура, single pointer, screen reader и browser zoom являются равноправными путями. Shortcut не заменяет обычную навигацию.

## 2. Landmarks, headings и skip navigation

Каждый route/workspace содержит:

1. skip link к `main`, видимый при focus;
2. один top-level `banner` с идентичностью текущего черновика (панель или страница) и подключения;
3. labelled primary `navigation`, если навигация присутствует;
4. один `main` с единственным page-level `h1`;
5. labelled `search` для каждого самостоятельного поиска;
6. labelled `complementary` для inspector/diagnostics, когда они находятся рядом с main content;
7. named `region` только для действительно самостоятельных областей; несколько однотипных landmarks получают уникальные локализуемые names.

DOM и heading order отражают смысловой порядок, а не визуальную раскладку grid/flex. Скрытая или collapsed панель не остаётся в accessibility tree. Route title и `document.title` обновляются через message IDs и включают текущий объект, но не секреты.

## 3. Focus order и управление

### Общие правила

- `Tab`/`Shift+Tab` перемещают focus между компонентами в DOM/task order. Положительные `tabindex` запрещены.
- Внутри composite widget (tabs, tree, listbox, menu, toolbar, data grid) используется documented APG pattern: один tab stop и arrow-key navigation/roving focus, если pattern этого требует.
- `Enter` активирует link/button-default action; `Space` активирует button/toggle/checkbox; `Escape` закрывает верхний dismissible layer без потери underlying state.
- Focus не переносится только из-за loading, validation или background refresh. После user-triggered navigation он переходит к route heading либо явной цели операции.
- После удаления выбранного элемента focus переходит к следующему логическому элементу, затем к предыдущему, затем к container action. После закрытия dialog/popover возвращается к opener, если он существует и доступен.
- Modal dialog удерживает focus и блокирует background; non-modal inspector/drawer не создаёт trap. Toast/status не получает focus автоматически.
- Sticky headers, banners, drawers и overlays не закрывают focused element; active element прокручивается в видимую область с учётом reduced motion.

### Canvas 72×64 и drag alternatives

- Canvas имеет доступный outline/list representation страниц и виджетов; пользователь может выбрать объект без pointer hit-testing.
- Move/resize/reorder доступны через keyboard commands и через single-pointer controls без удерживаемого drag. Inspector позволяет ввести точные `x/y/width/height` model units.
- Drag остаётся ускорителем. Он не является единственным путём и не меняет модель до валидного commit.
- Selection, coordinates, collision, out-of-bounds и commit/cancel объявляются краткими локализуемыми status messages; поток arrow-key событий не создаёт речевой шум на каждый visual pixel.
- Pan/zoom имеют кнопки и keyboard controls; browser zoom не перехватывается приложением.

## 4. Accessible names, descriptions и technical data

- Каждый control имеет programmatic name, совпадающий с видимой label по смыслу; icon-only action получает локализуемую accessible label и tooltip, но tooltip не является единственным name.
- Help text, format requirements и warning связываются с input через description relationship. Placeholder не заменяет label.
- User/server data (`dashboard name`, hostname, item key) не переводятся и включаются в accessible name только с безопасным локализованным context.
- IDs, macros, `key_`, версии, API field names и diagnostic IDs читаются как technical values; UI предоставляет отдельное copy action и не превращает их в длинную непонятную label каждого соседнего control.
- Decorative mascot/background art скрыты от accessibility tree. Смысловая иллюстрация получает короткий alt; surrounding prose не дублируется в alt.

## 5. Forms, errors и status messages

- Required/format constraints показаны до submit, связаны с полем и не передаются только цветом или символом без текста.
- Ошибка идентифицирует конкретное поле и описывает проблему текстом. Введённое значение сохраняется, кроме очищаемого секрета после безопасной передачи согласно connection contract.
- При нескольких ошибках появляется summary с links к полям; focus переходит в summary только после неуспешного user-triggered submit, затем пользователь сам выбирает поле.
- Inline error получает stable ID и связывается через `aria-describedby`; `aria-invalid` отражает текущее состояние, но не заменяет текст.
- Blocking error использует alert/heading только один раз. Неблокирующие progress/success/status updates используют polite status semantics и не прерывают текущую речь.
- `unknown_outcome` никогда не объявляется error/success по догадке: label сообщает неопределённость, write retry недоступен, доступен reconciliation.

## 6. Targets и concurrent input

- Product minimum для самостоятельной clickable/tappable области — 32×32 CSS px; standard control — 36 px, prominent action — 40 px. Это строже WCAG 2.2 AA minimum 24×24 CSS px.
- Compact 32 px controls допустимы в плотных таблицах/toolbars. Visible glyph может быть меньше, но hit area сохраняется и не перекрывает соседний target.
- Inline text links следуют line-height exception, но получают различимость помимо цвета и достаточное расстояние от соседних standalone actions.
- Hover-only actions запрещены. Любое действие доступно focus/touch; появление action на hover дублируется при `focus-within` и не меняет layout неожиданно.
- Приложение не блокирует переключение между keyboard, mouse, touch/stylus и assistive input.

## 7. Focus indicator

- Base treatment: сплошной outline не менее 2 CSS px вокруг minimum bounding box плюс 2 px offset, если граница компонента позволяет.
- Contrast indicator к adjacent colors и focused/unfocused change — не менее 3:1. `focus.ring` и `focus.offset` из P1-00C проверяются на каждой surface/status combination.
- Focus не обозначается только background/shadow/animation. В forced-colors используется system `Highlight`/`Canvas` и видимый outline.
- Selected/current/checked и focused — разные состояния; одновременно они остаются различимыми.

## 8. Zoom, text scaling и reflow

- При browser zoom/text enlargement 200% текст и controls не обрезаются, не перекрываются и сохраняют функции.
- Non-canvas application shell reflows до эквивалента 320 CSS px ширины: navigation/inspector становятся sequential panels/drawers, primary action и error summary остаются достижимыми без двухмерного scrolling.
- Двумерный scroll допускается внутри essential data table, structural diff и dashboard canvas, где spatial relationship потеряется при линейном reflow. Heading, filters, pagination, diagnostics и actions вокруг такой области reflow отдельно.
- Desktop design references: 1440×900 основной, 1024×720 compact. Они не являются запретом меньшего окна: ниже layout переключается в reflow mode.
- На 1280×1024 дополнительно проверяется 400% browser zoom (примерно 320 CSS px content width); на 200% проверяются 640×512 effective viewport и все critical flows.
- Sticky UI не занимает область так, чтобы focused control или error становились полностью скрытыми.

## 9. Motion, timing и loading

- Ни одна операция не требует animation для понимания результата. Auto-playing decorative motion не используется.
- При `prefers-reduced-motion: reduce` отключаются transform-based movement, parallax, animated pan/zoom, pulsing and non-essential smooth scrolling. State меняется сразу либо коротким non-spatial transition.
- Progress имеет текстовый stage/count, когда backend предоставляет их, и остаётся понятным при static rendering. Indeterminate animation не является единственным признаком работы.
- Skeleton не объявляет десятки fake controls screen reader; вместо этого используется один localized loading status.
- Critical confirmations не исчезают по таймеру. Toast содержит эквивалентную запись в persistent diagnostics/history, если результат нужен для продолжения работы.

## 10. Color, contrast и forced colors

- Text/background: 4.5:1 для обычного текста, 3:1 для крупного; interactive boundaries, icons и state indicators — минимум 3:1 там, где WCAG требует non-text contrast.
- Status/diff/editor meaning всегда содержит text/icon/shape, как определено P1-00C.
- При `forced-colors: active`: decorative gradients/images/shadows отключаются; используются `Canvas`, `CanvasText`, `ButtonFace`, `ButtonText`, `LinkText`, `GrayText`, `Highlight`, `HighlightText`.
- `forced-color-adjust: none` допускается только для essential spatial/data sample после отдельной contrast проверки; для обычных controls запрещён.
- Windows High Contrast Black и White входят в acceptance matrix; Linux проверяется с browser forced-colors support, desktop high-contrast theme и без предположения, что все дистрибутивы одинаковы.

## 11. Windows/Linux platform contract

| Область | Windows 10+ | Linux GUI | Общий инвариант |
|---|---|---|---|
| Sans font | System UI обычно Segoe UI | Browser/system UI может быть Noto Sans, Ubuntu, Cantarell, DejaVu Sans или иным distro font | Никаких pixel-exact line breaks; controls растут по content |
| Mono font | Cascadia Mono/Segoe UI Mono при наличии | Distro monospace | IDs wrap/copy без зависимости от glyph width |
| Focus/HC | Edge/Chrome + Windows High Contrast/forced colors | Firefox/Chromium + desktop/browser contrast facilities | Видимый focus и смысл без authored colors |
| Screen reader reference | Narrator и минимум один широко используемый дополнительный reader при доступности стенда | Orca на поддерживаемом desktop/browser combination | Native semantics/name/role/value; различия фиксируются, не маскируются |
| File/native dialogs | Platform dialog/portal определяется P1-IN-09 | Portal/GTK behavior определяется P1-IN-09 | Design не рисует браузерный path picker, которого нет |
| Scrollbars | System/browser metrics могут меняться | Theme/browser metrics могут меняться | Layout не полагается на фиксированную ширину scrollbar |

Поддерживаемая browser/AT matrix с точными версиями фиксируется P1-11/P6-04. Этот документ задаёт классы проверок, но не выдумывает уже испытанные комбинации.

## 12. Acceptance checklist для P1/P2

- [ ] Все actions достижимы keyboard-only без shortcut; tab order и composite keyboard model документированы.
- [ ] Canvas move/resize/reorder имеют keyboard и non-drag pointer alternatives.
- [ ] Landmarks/headings/names/descriptions проходят accessibility tree inspection.
- [ ] Error summary, inline errors, loading/progress, conflict и unknown outcome объявляются без focus theft/announcement storm.
- [ ] Targets соответствуют 32×32 product minimum; focus indicator проверен на каждой theme/status surface.
- [ ] 200% text/zoom и 400%/320 CSS px reflow проходят без потери critical action; essential 2D regions изолированы.
- [ ] Reduced motion и forced colors не теряют state/selection/focus/diff meaning.
- [ ] Light/dark/system, Windows High Contrast Black/White и Linux contrast setup проверены.
- [ ] EN/RU long text, platform fonts и 200% scaling не обрезают labels/errors.
- [ ] Результаты записаны как фактические browser/OS/AT observations; непроверенная комбинация не называется supported.

## 13. Первичные источники

- [WCAG 2.2](https://www.w3.org/TR/WCAG22/) и W3C Understanding: [Reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html), [Focus Appearance](https://www.w3.org/WAI/WCAG22/Understanding/focus-appearance), [Target Size](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum), [Dragging Movements](https://www.w3.org/WAI/WCAG22/Understanding/dragging-movements), [Error Identification](https://www.w3.org/WAI/WCAG22/Understanding/error-identification), [Labels or Instructions](https://www.w3.org/WAI/WCAG22/Understanding/labels-or-instructions).
- WAI-ARIA APG: [Keyboard Interface](https://www.w3.org/WAI/ARIA/apg/practices/keyboard-interface/) и [Landmark Regions](https://www.w3.org/WAI/ARIA/apg/practices/landmark-regions/).
- Microsoft Learn: [Windows high contrast mode](https://learn.microsoft.com/en-us/fluent-ui/web-components/design-system/high-contrast).
