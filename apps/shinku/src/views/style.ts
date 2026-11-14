// Poser un style calculé sur un élément (une largeur, un remplissage) sans
// jamais écrire d'attribut `style`.
//
// La politique de sécurité de contenu de Shinku interdit les styles écrits
// dans la page. Les attributs `style` sont donc refusés par le moteur web,
// y compris ceux qu'une bibliothèque poserait à notre place. Passer par les
// propriétés de l'élément, comme ici, reste permis.

import { noChange } from 'lit-html';
import { Directive, directive, type ElementPart, type PartInfo, PartType } from 'lit-html/directive.js';

type Properties = Readonly<Record<string, string>>;

class StyleProperties extends Directive {
  private applied: Properties = {};

  constructor(info: PartInfo) {
    super(info);
    if (info.type !== PartType.ELEMENT) {
      throw new Error('`styles` se place sur un élément : <div ${styles({ … })}>');
    }
  }

  render(_properties: Properties): unknown {
    return noChange;
  }

  override update(part: ElementPart, [properties]: [Properties]): unknown {
    const { style } = part.element as HTMLElement | SVGElement;
    for (const name of Object.keys(this.applied)) {
      if (!(name in properties)) {
        style.removeProperty(name);
      }
    }
    for (const [name, value] of Object.entries(properties)) {
      if (this.applied[name] !== value) {
        style.setProperty(name, value);
      }
    }
    this.applied = { ...properties };
    return noChange;
  }
}

/**
 * Pose des propriétés CSS sur un élément. Les noms sont ceux du CSS
 * (`stroke-dashoffset`, pas `strokeDashoffset`).
 */
export const styles = directive(StyleProperties);
