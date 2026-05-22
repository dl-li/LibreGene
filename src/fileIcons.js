import {
  Atom, Binoculars, Biohazard, Brain, CircuitBoard, Eclipse,
  FlaskConical, FlaskRound, Microscope, Orbit, Shell, Syringe,
  Telescope, TestTubeDiagonal,
  Birdhouse, Cannabis, Flower2, Leaf, MountainSnow, Rose, Shrub,
  Sprout, Stone, TreePalm, TreePine,
  Earth, FerrisWheel, ShipWheel, Ship, RollerCoaster, Snowflake, Sun,
  Astroid, Club, Spade, Heart,
  ZodiacAries, ZodiacCancer, ZodiacCapricorn, ZodiacGemini,
  ZodiacLeo, ZodiacLibra, ZodiacOphiuchus, ZodiacPisces,
  ZodiacSagittarius, ZodiacScorpio, ZodiacTaurus, ZodiacVirgo,
  Spool, Ribbon, Bot, Palette, Paintbrush, DraftingCompass,
  CableCar, FishingHook, Droplet, Rainbow, Zap, Flame, Moon,
  LifeBuoy, Bird, Gift,
} from 'lucide-react';

const ICONS = [
  Atom, Binoculars, Biohazard, Brain, CircuitBoard, Eclipse,
  FlaskConical, FlaskRound, Microscope, Orbit, Shell, Syringe,
  Telescope, TestTubeDiagonal,
  Birdhouse, Cannabis, Flower2, Leaf, MountainSnow, Rose, Shrub,
  Sprout, Stone, TreePalm, TreePine,
  Earth, FerrisWheel, ShipWheel, Ship, RollerCoaster, Snowflake, Sun,
  Astroid, Club, Spade, Heart,
  ZodiacAries, ZodiacCancer, ZodiacCapricorn, ZodiacGemini,
  ZodiacLeo, ZodiacLibra, ZodiacOphiuchus, ZodiacPisces,
  ZodiacSagittarius, ZodiacScorpio, ZodiacTaurus, ZodiacVirgo,
  Spool, Ribbon, Bot, Palette, Paintbrush, DraftingCompass,
  CableCar, FishingHook, Droplet, Rainbow, Zap, Flame, Moon,
  LifeBuoy, Bird, Gift,
];

export function hashFileName(name) {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = ((hash << 5) - hash) + name.charCodeAt(i);
    hash |= 0;
  }
  return Math.abs(hash) % ICONS.length;
}

export function getFileIcon(filename) {
  return ICONS[hashFileName(filename)] || Atom;
}
