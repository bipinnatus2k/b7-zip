pub mod platforms;

use crate::platforms::{platform_linux, platform_windows};
use gpui::prelude::FluentBuilder;
use gpui::{Action, AnyElement, App, Context, Decorations, ElementId, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, StatefulInteractiveElement, Styled, Window, WindowButtonLayout, WindowControlArea, actions, div, hsla, px};
use smallvec::SmallVec;
use std::mem;
use ui::components::stack::{h_flex, v_flex};
use ui::styles::decoration::CLIENT_SIDE_DECORATION_ROUNDING;
use ui::styles::platform::PlatformStyle;
use ui::traits::styled_ext::StyledExt;
use ui::utils::constants::{TRAFFIC_LIGHT_PADDING, platform_title_bar_height};

actions!(
    titlebar,[
        CloseWindow
    ]
);

/// Default title bar height when none is supplied.
pub const DEFAULT_TITLE_BAR_HEIGHT: f32 = 34.;

/// Rebuilds a title bar child on every render. Elements are single-use in
/// gpui, so persistent content must be produced by a factory instead of
/// being stored as an element.
pub type TitleBarContent = Box<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>;

pub struct TitleBarStyle {
    /// Background color of the title bar. Supplied by the caller since
    /// this crate does not depend on a theme system.
    background: Hsla,
    inactive_background: Hsla,
    /// Height of the title bar. Defaults to [`DEFAULT_TITLE_BAR_HEIGHT`].
    height: Pixels,
}

impl Default for TitleBarStyle {
    fn default() -> Self {
        let bg = hsla(215. / 360., 12. / 100., 15. / 100., 1.);
        Self {
            background: bg,
            inactive_background: bg,
            height: Pixels::from(DEFAULT_TITLE_BAR_HEIGHT),
        }
    }
}

pub struct PlatformTitleBar {
    id: ElementId,
    children: SmallVec<[AnyElement; 2]>,
    /// Persistent content, rebuilt on every render (see [`TitleBarContent`]).
    content: Option<TitleBarContent>,
    style: TitleBarStyle,
    platform_style: PlatformStyle,
    should_move: bool,
    show_left_controls: bool,
    show_right_controls: bool,
    button_layout: Option<WindowButtonLayout>,
    /// Action dispatched when the close button is clicked.
    close_action: Box<dyn Action>,
}

impl PlatformTitleBar {
    pub fn init() {


    }

    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: SmallVec::new(),
            content: None,
            style: TitleBarStyle::default(),
            platform_style: PlatformStyle::platform(),
            should_move: false,
            show_left_controls: true,
            show_right_controls: true,
            button_layout: None,
            close_action: CloseWindow.boxed_clone(),
        }
    }

    pub fn background(mut self, background: Hsla) -> Self {
        self.style.background = background;
        self
    }

    pub fn set_background(&mut self, background: Hsla) {
        self.style.background = background;
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.style.height = height;
        self
    }

    pub fn set_height(&mut self, height: Pixels) {
        self.style.height = height;
    }

    pub fn close_action(mut self, action: Box<dyn Action>) -> Self {
        self.close_action = action;
        self
    }

    // pub fn set_close_action(&mut self, action: Option<Box<dyn Action>>) {
    //     self.close_action = action;
    // }

    pub fn set_button_layout(&mut self, button_layout: Option<WindowButtonLayout>) {
        self.button_layout = button_layout;
    }

    pub fn set_children<T>(&mut self, children: T)
    where
        T: IntoIterator<Item = AnyElement>,
    {
        self.children = children.into_iter().collect();
    }

    /// Registers persistent content that is rebuilt on every render.
    ///
    /// Unlike [`Self::set_children`] (consumed by the first render, the
    /// upstream contract is for the owner to re-supply them each frame),
    /// this survives any number of re-renders.
    pub fn content(
        mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.content = Some(Box::new(content));
        self
    }

    pub fn set_content(
        &mut self,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) {
        self.content = Some(Box::new(content));
    }

    fn effective_button_layout(
        &self,
        decorations: &Decorations,
        cx: &App,
    ) -> Option<WindowButtonLayout> {
        if self.platform_style == PlatformStyle::Linux
            && matches!(decorations, Decorations::Client { .. })
        {
            self.button_layout.or_else(|| cx.button_layout())
        } else {
            None
        }
    }

    fn title_bar_color(&self, window: &mut Window) -> Hsla {
        if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            if window.is_window_active() && !self.should_move {
                self.style.background
            } else {
                self.style.inactive_background
            }
        } else {
            self.style.background
        }
    }

}

/// Renders the platform-appropriate left-side window controls (e.g. Ubuntu/GNOME close button).
///
/// Only relevant on Linux with client-side decorations when the window manager
/// places controls on the left.
pub fn render_left_window_controls(
    button_layout: Option<WindowButtonLayout>,
    close_action: Box<dyn Action>,
    window: &Window,
) -> Option<AnyElement> {
    if PlatformStyle::platform() != PlatformStyle::Linux {
        return None;
    }
    if !matches!(window.window_decorations(), Decorations::Client { .. }) {
        return None;
    }
    let button_layout = button_layout?;
    if button_layout.left[0].is_none() {
        return None;
    }
    Some(
        platform_linux::LinuxWindowControls::new(
            "left-window-controls",
            button_layout.left,
            close_action,
        )
            .into_any_element(),
    )
}

/// Renders the platform-appropriate right-side window controls (close, minimize, maximize).
///
/// Returns `None` on Mac or when the platform doesn't need custom controls
/// (e.g. Linux with server-side decorations).
pub fn render_right_window_controls(
    button_layout: Option<WindowButtonLayout>,
    close_action: Box<dyn Action>,
    window: &Window,
) -> Option<AnyElement> {
    let decorations = window.window_decorations();
    let height = platform_title_bar_height(window);

    match PlatformStyle::platform() {
        PlatformStyle::Linux => {
            if !matches!(decorations, Decorations::Client { .. }) {
                return None;
            }
            let button_layout = button_layout?;
            if button_layout.right[0].is_none() {
                return None;
            }
            Some(
                platform_linux::LinuxWindowControls::new(
                    "right-window-controls",
                    button_layout.right,
                    close_action,
                )
                    .into_any_element(),
            )
        }
        PlatformStyle::Windows => {
            Some(platform_windows::WindowsWindowControls::new(height).into_any_element())
        }
        PlatformStyle::Mac => None,
    }
}


impl Render for PlatformTitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let supported_controls = window.window_controls();
        let decorations = window.window_decorations();
        let height = platform_title_bar_height(window);
        let titlebar_color = self.title_bar_color(window);
        let children = mem::take(&mut self.children);

        let button_layout = self.effective_button_layout(&decorations, cx);
        // let sidebar = self.sidebar_render_state(cx);

        let title_bar = h_flex()
            .window_control_area(WindowControlArea::Drag)
            .w_full()
            .h(height)
            .map(|this| {
                this.on_mouse_down_out(cx.listener(move |this, _ev, _window, _cx| {
                    this.should_move = false;
                }))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _ev, _window, _cx| {
                            this.should_move = false;
                        }),
                    )
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _ev, _window, _cx| {
                            this.should_move = true;
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, _ev, window, _| {
                        if this.should_move {
                            this.should_move = false;
                            window.start_window_move();
                        }
                    }))
            })
            .map(|this| {
                // Note: On Windows the title bar behavior is handled by the platform implementation.
                this.id(self.id.clone())
                    .when(self.platform_style == PlatformStyle::Mac, |this| {
                        this.on_click(|event, window, _| {
                            if event.click_count() == 2 {
                                window.titlebar_double_click();
                            }
                        })
                    })
                    .when(
                        self.platform_style == PlatformStyle::Linux
                            && supported_controls.maximize
                            && window.is_resizable(),
                        |this| {
                            this.on_click(|event, window, _| {
                                if event.click_count() == 2 {
                                    window.zoom_window();
                                }
                            })
                        },
                    )
            })
            .map(|this| {
                let show_left_controls = self.show_left_controls;

                if window.is_fullscreen() {
                    this.pl_2()
                } else if self.platform_style == PlatformStyle::Mac && show_left_controls {
                    this.pl(px(TRAFFIC_LIGHT_PADDING))
                } else if let Some(controls) = show_left_controls
                    .then(|| {
                        render_left_window_controls(
                            button_layout,
                            self.close_action.boxed_clone(),
                            window,
                        )
                    })
                    .flatten()
                {
                    this.child(controls)
                } else {
                    this.pl_2()
                }
            })
            .map(|el| match decorations {
                Decorations::Server => el,
                Decorations::Client { tiling, .. } => el
                    .when(
                        !(tiling.top || tiling.right),
                        |el| el.rounded_tr(CLIENT_SIDE_DECORATION_ROUNDING),
                    )
                    .when(
                        !(tiling.top || tiling.left),
                        |el| el.rounded_tl(CLIENT_SIDE_DECORATION_ROUNDING),
                    )
                    // this border is to avoid a transparent gap in the rounded corners
                    .mt(px(-1.))
                    .mb(px(-1.))
                    .border(px(1.))
                    .border_color(titlebar_color),
            })
            .bg(titlebar_color)
            .content_stretch()
            .child(
                div()
                    .id(self.id.clone())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .overflow_x_hidden()
                    .w_full()
                    .children(children),
            )
            .when(
                !window.is_fullscreen(),
                |title_bar| {
                    let title_bar = title_bar.children(
                        self.show_right_controls
                            .then(|| {
                                render_right_window_controls(
                                    button_layout,
                                    self.close_action.boxed_clone(),
                                    window,
                                )
                            })
                            .flatten(),
                    );

                    if self.platform_style == PlatformStyle::Linux
                        && matches!(decorations, Decorations::Client { .. })
                    {
                        title_bar.when(supported_controls.window_menu, |titlebar| {
                            titlebar.on_mouse_down(MouseButton::Right, move |ev, window, _| {
                                window.show_window_menu(ev.position)
                            })
                        })
                    } else {
                        title_bar
                    }
                },
            );

        v_flex()
            .w_full()
            .child(title_bar)
            // .child(self.system_window_tabs.clone().into_any_element())
    }
}


impl ParentElement for PlatformTitleBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements)
    }
}
