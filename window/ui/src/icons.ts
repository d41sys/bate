import {
  BookOpen, Bot, Brain, Bug, Calendar, Camera, ChartLine, CodeXml, Compass, Database, FlaskConical, Folder,
  Globe, KeyRound, Leaf, Mail, Music, PenLine, Rocket, Search, Shield, Sprout, SquareTerminal, Wrench, Zap,
} from "lucide-react";

/** The icons a bot can have, by the name kept as its avatar (the service allows 8 characters). A Map: an avatar is data from a bot file, and must not find `constructor`. */
export const BOT_ICONS = new Map(Object.entries({
  bot: Bot, search: Search, pen: PenLine, code: CodeXml, terminal: SquareTerminal, folder: Folder, compass: Compass, brain: Brain,
  book: BookOpen, bug: Bug, chart: ChartLine, data: Database, globe: Globe, shield: Shield, wrench: Wrench, rocket: Rocket,
  flask: FlaskConical, mail: Mail, calendar: Calendar, camera: Camera, music: Music, sprout: Sprout, leaf: Leaf, zap: Zap, key: KeyRound,
}));
