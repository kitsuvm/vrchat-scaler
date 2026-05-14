#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![windows_subsystem = "windows"]

use iced::{
    Alignment, Border, Color, Font, Length, Padding, Shadow, Task,
    border::Radius,
    font, system,
    theme::{self, Mode},
    widget::{Column, button, column, container, space::vertical, text, text_input},
    window,
};
use rosc::{OscMessage, OscPacket, OscType};
use std::{net::UdpSocket, sync::Arc};

/// The address of the VRChat OSC listener. This is where we'll send our UDP packets.
static VRCHAT_ADDR: &str = "127.0.0.1:9000";

/// The local address to bind our UDP socket to. Using port 0 allows the OS to choose an available port.
static LOCAL_ADDR: &str = "127.0.0.1:0";

/// This struct holds the state of our application, including the input text and the UDP socket for sending messages.
#[derive(Debug, Default, Clone)]
struct State {
    /// The text currently in the input field.
    input_text: String,
    /// The UDP socket used to send messages to VRChat.
    connection: Connection,
    /// A flag to indicate if the input text is invalid (e.g., not a valid float). This can be used to provide user feedback in the UI.
    input_error: InputError,
    /// The current system theme (light or dark) to ensure the UI matches the user's preferences.
    system_theme: Mode,
}

/// Represents the connection status to VRChat's OSC listener.
#[derive(Debug, Clone, Default)]
enum Connection {
    /// Represents a successful connection, holding the UDP socket.
    Connected(Arc<UdpSocket>),
    /// Represents an ongoing attempt to connect to VRChat's OSC listener.
    Connecting,
    /// Default state when not connected or trying to connect.
    #[default]
    Disconnected,
}

/// The messages that can be sent from the view to the update function.
#[derive(Debug, Clone, Default)]
enum Message {
    /// Represents a change in the input text field.
    InputChanged(String),
    /// Represents the action of sending the message to VRChat.
    Send,
    /// Represents the action of attempting to connect to VRChat's OSC listener.
    Connect,
    /// Represents a message indicating that the connection to VRChat's OSC listener has been established.
    Connected(Arc<UdpSocket>),
    /// Represents the action of disconnecting from VRChat's OSC listener.
    Disconnect,
    /// Represents a message to change the application's theme based on the system theme.
    SetTheme(Mode),
    /// None represents a no-op message, used when no action is needed.
    #[default]
    None,
}

/// Represents errors that can occur during input validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum InputError {
    /// Represents an error when the input cannot be parsed as a float.
    NotAFloat,
    /// Represents an error when the input value is out of the acceptable range (0.01 - 10000).
    OutOfRange,
    /// None represents no error, used when the input is valid.
    #[default]
    None,
}

/// The main function initializes the application and starts the event loop.
fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SCALER_OSC_LOG")
                .unwrap_or_else(|_| "scaler_osc=info".into()),
        )
        .init();

    iced::application(boot, update, view)
        .title("VRChat Scaler")
        .window(window::Settings {
            size: (250, 250).into(),
            min_size: Some((250, 250).into()),
            ..Default::default()
        })
        .run()
}

/// The boot function initializes the application state and starts the connection process to VRChat's OSC listener.
fn boot() -> (State, Task<Message>) {
    (
        State::default(),
        Task::batch([
            Task::perform(connect(), |msg| msg),
            system::theme().map(Message::SetTheme),
        ]),
    )
}

/// The update function processes incoming messages and updates the application state accordingly.
fn update(app: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::InputChanged(val) => {
            app.input_text = val;
            Task::none()
        }
        Message::Send => match &app.connection {
            Connection::Connected(socket) => {
                tracing::info!("Sending message to VRChat: {}", app.input_text);
                app.input_error = InputError::None; // Reset the input error before processing the input

                let text = app.input_text.clone();
                let socket = socket.clone();

                match text.parse::<f32>() {
                    Ok(v) => {
                        if (0.01..=10000.0).contains(&v) {
                            Task::perform(
                                async move {
                                    let packet = OscPacket::Message(OscMessage {
                                        addr: "/avatar/eyeheight".to_string(),
                                        args: vec![OscType::Float(v)],
                                    });

                                    match rosc::encoder::encode(&packet) {
                                        Ok(msg_buf) => {
                                            if let Err(e) = socket.send(&msg_buf) {
                                                tracing::error!("Failed to send OSC packet: {}", e);
                                                Message::Disconnect
                                            } else {
                                                tracing::info!("Message sent successfully.");
                                                Message::None
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to encode OSC packet: {}", e);
                                            Message::None
                                        }
                                    }
                                },
                                |msg| msg,
                            )
                        } else {
                            tracing::error!("Input value out of range (0.01 - 10000): {}", v);
                            app.input_error = InputError::OutOfRange;
                            Task::none()
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to parse input as float: {}", e);
                        app.input_error = InputError::NotAFloat;
                        Task::none()
                    }
                }
            }
            Connection::Disconnected | Connection::Connecting => {
                tracing::error!("UDP socket not available. Cannot send message.");
                Task::none()
            }
        },
        Message::Connect => {
            if let Connection::Disconnected = app.connection {
                tracing::info!("Attempting to connect to VRChat OSC listener...");
                app.connection = Connection::Connecting;

                Task::perform(connect(), |msg| msg)
            } else {
                tracing::warn!("Already connected or connecting. Ignoring connect request.");
                Task::none()
            }
        }
        Message::Connected(socket) => {
            tracing::info!("Successfully connected to VRChat OSC listener.");
            app.connection = Connection::Connected(socket);
            Task::none()
        }
        Message::Disconnect => {
            tracing::info!("Disconnecting from VRChat OSC listener.");
            app.connection = Connection::Disconnected;
            Task::none()
        }
        Message::SetTheme(theme) => {
            tracing::info!("System theme changed: {:?}", theme);
            app.system_theme = theme;
            Task::none()
        }
        Message::None => Task::none(),
    }
}

/// The view function renders the user interface based on the current application state.
fn view(app: &State) -> Column<'_, Message, AmoledTheme> {
    match &app.connection {
        Connection::Disconnected => column![
            button(text("Connect to VRChat").font(Font {
                weight: font::Weight::Bold,
                ..Default::default()
            }))
            .on_press(Message::Connect)
        ]
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill),

        Connection::Connecting => column![text("Connecting to VRChat...").font(Font {
            weight: font::Weight::Bold,
            ..Default::default()
        })]
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill),

        Connection::Connected(_) => column![
            text("VRChat Scaler").size(30).font(Font {
                weight: font::Weight::Bold,
                ..Default::default()
            }),
            vertical().height(Length::Fixed(10.0)),
            container(
                text_input("1.73", &app.input_text)
                    .on_input(Message::InputChanged)
                    .on_submit(Message::Send)
                    .padding(10),
            )
            .max_width(300),
            match app.input_error {
                InputError::NotAFloat => Some(
                    text("Please enter a valid float")
                        .size(14)
                        .center()
                        .color([1.0, 0.0, 0.0])
                ),
                InputError::OutOfRange => Some(
                    text("Please enter a value between 0.01 and 10000")
                        .size(14)
                        .center()
                        .color([1.0, 0.0, 0.0])
                ),
                InputError::None => None,
            },
            vertical().height(Length::Fixed(10.0)),
            button(
                text("Send to VRChat")
                    .font(Font {
                        weight: font::Weight::Bold,
                        ..Font::default()
                    })
                    .size(18)
                    .color(Color::from_rgb8(0xFF, 0xFF, 0xFF))
            )
            .padding(Padding::new(20.0).vertical(10.0))
            .on_press(Message::Send)
        ]
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill),
    }
}

/// This asynchronous function attempts to create a UDP socket, bind it to the local address, and connect it to the VRChat OSC listener. It returns a `Message` indicating the result of the connection attempt.
async fn connect() -> Message {
    if let Ok(socket) = UdpSocket::bind(LOCAL_ADDR)
        .inspect_err(|e| tracing::error!("Failed to bind UDP socket: {}", e))
        .and_then(|s| {
            s.connect(VRCHAT_ADDR)
                .inspect_err(|e| tracing::error!("Failed to connect UDP socket: {}", e))
                .map(|_| s)
        })
    {
        tracing::info!("UDP socket bound to {LOCAL_ADDR}");
        Message::Connected(Arc::new(socket))
    } else {
        Message::Disconnect
    }
}

/// The [`AmoledTheme`] struct defines a custom theme for the application, providing styles for various UI components based on the system theme (light or dark). It implements the necessary traits to be used as a theme in the Iced application.
struct AmoledTheme {
    /// The current mode of the theme, which can be light or dark.
    mode: theme::Mode,
    /// The base style for the theme, defining background and text colors.
    style: theme::Style,
    /// The color palette for the theme, providing specific colors for different UI elements.
    palette: theme::Palette,
    /// The name of the theme, used for display purposes.
    name: &'static str,
}

impl theme::Base for AmoledTheme {
    fn default(preference: Mode) -> Self {
        match preference {
            Mode::Light | Mode::None => Self {
                mode: Mode::Light,
                style: theme::Style {
                    background_color: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                    text_color: Color::from_rgb8(0x00, 0x00, 0x00),
                },
                palette: theme::Palette {
                    background: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                    text: Color::from_rgb8(0x00, 0x00, 0x00),
                    primary: Color::from_rgb8(0x85, 0x00, 0xFF),
                    success: Color::from_rgb8(0x00, 0xFF, 0x00),
                    warning: Color::from_rgb8(0xFF, 0xFF, 0x00),
                    danger: Color::from_rgb8(0xFF, 0x00, 0x00),
                },
                name: "Amoled Light",
            },
            Mode::Dark => Self {
                mode: Mode::Dark,
                style: theme::Style {
                    background_color: Color::from_rgb8(0x00, 0x00, 0x00),
                    text_color: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                },
                palette: theme::Palette {
                    background: Color::from_rgb8(0x00, 0x00, 0x00),
                    text: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                    primary: Color::from_rgb8(0x85, 0x00, 0xFF),
                    success: Color::from_rgb8(0x00, 0xFF, 0x00),
                    warning: Color::from_rgb8(0xFF, 0xFF, 0x00),
                    danger: Color::from_rgb8(0xFF, 0x00, 0x00),
                },
                name: "Amoled Dark",
            },
        }
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn base(&self) -> theme::Style {
        self.style
    }

    fn palette(&self) -> Option<theme::Palette> {
        Some(self.palette)
    }

    fn name(&self) -> &str {
        self.name
    }
}

impl text::Catalog for AmoledTheme {
    type Class<'a> = Box<dyn Fn(&AmoledTheme) -> text::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme: &AmoledTheme| text::Style {
            color: Some(theme.style.text_color),
        })
    }

    fn style(&self, class: &Self::Class<'_>) -> text::Style {
        class(self)
    }
}

impl button::Catalog for AmoledTheme {
    type Class<'a> = Box<dyn Fn(&AmoledTheme, button::Status) -> button::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(
            |theme: &AmoledTheme, status: button::Status| button::Style {
                background: Some(iced::Background::Color(match status {
                    button::Status::Pressed => Color::from_rgb8(0x70, 0x00, 0xFF),
                    button::Status::Disabled => Color::from_rgb8(0x40, 0x00, 0x80),
                    button::Status::Hovered => Color::from_rgb8(0xA0, 0x00, 0xFF),
                    _ => theme.palette.primary,
                })),
                border: Border {
                    width: 0.0,
                    color: theme.palette.primary,
                    radius: Radius::new(10.0),
                },
                text_color: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                snap: true,
                shadow: Shadow {
                    offset: iced::Vector::new(0.0, 0.0),
                    color: Color::from_rgb8(0x00, 0x00, 0x00),
                    blur_radius: 0.0,
                },
            },
        )
    }

    fn style(&self, class: &Self::Class<'_>, status: button::Status) -> button::Style {
        class(self, status)
    }
}

impl text_input::Catalog for AmoledTheme {
    type Class<'a> = Box<dyn Fn(&AmoledTheme, text_input::Status) -> text_input::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(
            |theme: &AmoledTheme, status: text_input::Status| text_input::Style {
                background: iced::Background::Color(match (status, theme.mode) {
                    (text_input::Status::Focused { is_hovered: _ }, Mode::Dark) => {
                        Color::from_rgb8(0x20, 0x20, 0x20)
                    }
                    (text_input::Status::Focused { is_hovered: _ }, _) => {
                        Color::from_rgb8(0xF0, 0xF0, 0xF0)
                    }
                    (text_input::Status::Disabled, _) => Color::from_rgb8(0x10, 0x10, 0x10),
                    (text_input::Status::Active, Mode::Dark) => Color::from_rgb8(0x10, 0x10, 0x10),
                    (text_input::Status::Active, _) => Color::from_rgb8(0xF0, 0xF0, 0xF0),
                    (text_input::Status::Hovered, Mode::Dark) => Color::from_rgb8(0x30, 0x30, 0x30),
                    (text_input::Status::Hovered, _) => Color::from_rgb8(0xE0, 0xE0, 0xE0),
                }),
                border: Border {
                    width: 2.0,
                    color: match theme.mode {
                        Mode::Dark => Color::from_rgb8(0x40, 0x40, 0x40),
                        _ => Color::from_rgb8(0xC0, 0xC0, 0xC0),
                    },
                    radius: Radius::new(10.0),
                },
                icon: Color::from_rgb8(0xFF, 0xFF, 0xFF),
                placeholder: Color::from_rgb8(0x80, 0x80, 0x80),
                value: theme.style.text_color,
                selection: theme.palette.primary,
            },
        )
    }

    fn style(&self, class: &Self::Class<'_>, status: text_input::Status) -> text_input::Style {
        class(self, status)
    }
}

impl container::Catalog for AmoledTheme {
    type Class<'a> = Box<dyn Fn(&AmoledTheme) -> container::Style + 'a>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme: &AmoledTheme| container::Style {
            background: Some(iced::Background::Color(theme.style.background_color)),
            border: Border {
                width: 0.0,
                color: theme.palette.primary,
                radius: Radius::new(0.0),
            },
            ..Default::default()
        })
    }

    fn style(&self, class: &Self::Class<'_>) -> container::Style {
        class(self)
    }
}
